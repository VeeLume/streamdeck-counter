//! StopwatchAdapter — owns stopwatch state and the tick thread.
//! Mirror of TimerAdapter for elapsed (counts up, no expiry).

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_time_hhmmss;
use crate::topics::{STOPWATCH_CTL, StopwatchControl};

const TICK_MS: u64 = 100;

pub struct StopwatchAdapter;

impl AdapterStatic for StopwatchAdapter {
    const NAME: &'static str = "stopwatch_adapter";
}

impl Adapter for StopwatchAdapter {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn policy(&self) -> StartPolicy {
        StartPolicy::Eager
    }
    fn topics(&self) -> &'static [&'static str] {
        &[STOPWATCH_CTL.name]
    }

    fn start(
        &self,
        cx: &Context,
        _bus: Arc<dyn Bus>,
        rx: Receiver<Arc<ErasedTopic>>,
    ) -> AdapterResult {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_thread = Arc::clone(&cancel);
        let cx = cx.clone();

        let join = std::thread::spawn(move || {
            let state: Mutex<HashMap<String, StopwatchEntry>> = Mutex::new(HashMap::new());

            loop {
                match rx.recv_timeout(Duration::from_millis(TICK_MS)) {
                    Ok(ev) => {
                        if let Some(ctl) = ev.downcast::<StopwatchControl>(STOPWATCH_CTL) {
                            handle_ctl(&cx, &state, ctl);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if cancel_for_thread.load(Ordering::Relaxed) {
                    break;
                }
                tick_all(&cx, &state);
            }
        });

        Ok(AdapterHandle::from_thread(join, move || {
            cancel.store(true, Ordering::Relaxed);
        }))
    }
}

#[derive(Clone)]
struct StopwatchEntry {
    elapsed_ms: u64,
    /// Some(unix_ms) = running; elapsed_ms is as-of that instant.
    anchor_unix_ms: Option<u64>,
    last_rendered_sec: Option<u64>,
}

fn handle_ctl(cx: &Context, state: &Mutex<HashMap<String, StopwatchEntry>>, ctl: &StopwatchControl) {
    let mut s = state.lock().unwrap();
    match ctl {
        StopwatchControl::Hello { ctx_id } => {
            if !s.contains_key(ctx_id) {
                let entry = load_from_globals(cx, ctx_id).unwrap_or(StopwatchEntry {
                    elapsed_ms: 0,
                    anchor_unix_ms: None,
                    last_rendered_sec: None,
                });
                s.insert(ctx_id.clone(), entry);
            }
            let entry = s.get_mut(ctx_id).unwrap();
            if let Some(anchor) = entry.anchor_unix_ms {
                let extra = unix_now_ms().saturating_sub(anchor);
                entry.elapsed_ms = entry.elapsed_ms.saturating_add(extra);
                entry.anchor_unix_ms = Some(unix_now_ms());
            }
            render_entry(cx, ctx_id, entry);
            persist(cx, ctx_id, entry);
        }
        StopwatchControl::Toggle { ctx_id } => {
            if let Some(entry) = s.get_mut(ctx_id) {
                if let Some(anchor) = entry.anchor_unix_ms.take() {
                    let extra = unix_now_ms().saturating_sub(anchor);
                    entry.elapsed_ms = entry.elapsed_ms.saturating_add(extra);
                } else {
                    entry.anchor_unix_ms = Some(unix_now_ms());
                }
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
        StopwatchControl::Reset { ctx_id } => {
            if let Some(entry) = s.get_mut(ctx_id) {
                entry.elapsed_ms = 0;
                entry.anchor_unix_ms = None;
                entry.last_rendered_sec = None;
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
    }
}

fn tick_all(cx: &Context, state: &Mutex<HashMap<String, StopwatchEntry>>) {
    let mut s = state.lock().unwrap();
    let now = unix_now_ms();

    for (ctx_id, entry) in s.iter_mut() {
        let Some(anchor) = entry.anchor_unix_ms else {
            continue;
        };
        let extra = now.saturating_sub(anchor);
        if extra == 0 {
            continue;
        }
        entry.elapsed_ms = entry.elapsed_ms.saturating_add(extra);
        entry.anchor_unix_ms = Some(now);

        let cur_sec = entry.elapsed_ms / 1000;
        if Some(cur_sec) != entry.last_rendered_sec {
            render_entry(cx, ctx_id, entry);
        }
    }
}

fn render_entry(cx: &Context, ctx_id: &str, entry: &mut StopwatchEntry) {
    let secs = entry.elapsed_ms / 1000;
    entry.last_rendered_sec = Some(secs);
    render_time_hhmmss(cx, ctx_id, secs);
}

fn persist(cx: &Context, ctx_id: &str, entry: &StopwatchEntry) {
    cx.globals().with_mut(|m| {
        let map = m
            .entry("stopwatches")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut e = Map::new();
        e.insert("elapsed_ms".into(), entry.elapsed_ms.into());
        if let Some(anchor) = entry.anchor_unix_ms {
            e.insert("anchor_unix_ms".into(), anchor.into());
        }
        map.insert(ctx_id.to_string(), Value::Object(e));
    });
}

fn load_from_globals(cx: &Context, ctx_id: &str) -> Option<StopwatchEntry> {
    let stopwatches = cx.globals().get("stopwatches")?;
    let v = stopwatches.get(ctx_id)?;
    let mut elapsed_ms = v.get("elapsed_ms").and_then(|n| n.as_u64()).unwrap_or(0);
    let mut anchor = v.get("anchor_unix_ms").and_then(|n| n.as_u64());
    if let Some(a) = anchor {
        let extra = unix_now_ms().saturating_sub(a);
        elapsed_ms = elapsed_ms.saturating_add(extra);
        anchor = Some(unix_now_ms());
    }
    Some(StopwatchEntry {
        elapsed_ms,
        anchor_unix_ms: anchor,
        last_rendered_sec: None,
    })
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}
