use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_time_hhmmss;

const TICK_MS: u64 = 100;

// ── Globals persistence ──────────────────────────────────────────────────────

/// State stored in cx.globals() so it survives instance teardown.
/// Key: cx.globals()["stopwatches"][ctx_id]
///
/// When paused:  { "elapsed_ms": u64 }
/// When running: { "elapsed_ms": u64, "anchor_unix_ms": u64 }
///   where elapsed = elapsed_ms + (now - anchor_unix_ms)
fn load_state(cx: &Context, ctx_id: &str) -> (u64, bool) {
    cx.globals()
        .get("stopwatches")
        .and_then(|v| v.get(ctx_id).cloned())
        .map(|entry| {
            let elapsed_base = entry
                .get("elapsed_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if let Some(anchor) = entry.get("anchor_unix_ms").and_then(|v| v.as_u64()) {
                let now = unix_now_ms();
                let extra = now.saturating_sub(anchor);
                (elapsed_base.saturating_add(extra), true)
            } else {
                (elapsed_base, false)
            }
        })
        .unwrap_or((0, false))
}

fn save_paused(cx: &Context, ctx_id: &str, elapsed_ms: u64) {
    cx.globals().with_mut(|m| {
        let map = m
            .entry("stopwatches")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut entry = Map::new();
        entry.insert("elapsed_ms".into(), elapsed_ms.into());
        map.insert(ctx_id.to_string(), Value::Object(entry));
    });
}

fn save_running(cx: &Context, ctx_id: &str, elapsed_ms: u64) {
    let anchor = unix_now_ms();
    cx.globals().with_mut(|m| {
        let map = m
            .entry("stopwatches")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut entry = Map::new();
        entry.insert("elapsed_ms".into(), elapsed_ms.into());
        entry.insert("anchor_unix_ms".into(), anchor.into());
        map.insert(ctx_id.to_string(), Value::Object(entry));
    });
}

fn clear_state(cx: &Context, ctx_id: &str) {
    cx.globals().with_mut(|m| {
        if let Some(map) = m.get_mut("stopwatches").and_then(|v| v.as_object_mut()) {
            map.remove(ctx_id);
        }
    });
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ── Action ───────────────────────────────────────────────────────────────────

pub struct StopwatchAction {
    elapsed_ms: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    epoch_seq: u64,
    running: bool,

    // Long-press tracking
    down_at: Option<Instant>,
    holding: Arc<AtomicBool>,
    press_seq: u64,
    active_press_id: Arc<AtomicU64>,
    long_fired_press_id: Arc<AtomicU64>,

    long_press_ms: u64,
}

impl Default for StopwatchAction {
    fn default() -> Self {
        Self {
            elapsed_ms: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: Arc::new(AtomicU64::new(0)),
            epoch_seq: 0,
            running: false,

            down_at: None,
            holding: Arc::new(AtomicBool::new(false)),
            press_seq: 0,
            active_press_id: Arc::new(AtomicU64::new(0)),
            long_fired_press_id: Arc::new(AtomicU64::new(0)),

            long_press_ms: 500,
        }
    }
}

impl ActionStatic for StopwatchAction {
    const ID: &'static str = super::ids::STOPWATCH;
}

impl Action for StopwatchAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        // Restore persisted state before requesting settings
        let (elapsed_ms, was_running) = load_state(cx, ctx_id);
        self.elapsed_ms.store(elapsed_ms, Ordering::Relaxed);
        self.running = was_running;

        let elapsed_secs = elapsed_ms / 1000;
        render_time_hhmmss(cx, ctx_id, elapsed_secs);

        if was_running {
            // Update the persisted anchor so elapsed continues from now
            save_running(cx, ctx_id, elapsed_ms);
            self.start_tick(cx, ctx_id);
        }

        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, _cx: &Context, ev: &incoming::DidReceiveSettings) {
        self.long_press_ms = parse_settings(&ev.settings);
    }

    fn teardown(&mut self, cx: &Context, ctx_id: &str) {
        self.stop_tick();
        // Persist current state on teardown (profile switch / SD exit)
        let elapsed_ms = self.elapsed_ms.load(Ordering::Relaxed);
        if self.running {
            save_running(cx, ctx_id, elapsed_ms);
        } else {
            save_paused(cx, ctx_id, elapsed_ms);
        }
    }

    fn key_down(&mut self, cx: &Context, ev: &incoming::KeyDown) {
        self.down_at = Some(Instant::now());
        self.holding.store(true, Ordering::SeqCst);

        self.press_seq = self.press_seq.wrapping_add(1);
        let pid = self.press_seq;
        self.active_press_id.store(pid, Ordering::SeqCst);
        self.long_fired_press_id.store(0, Ordering::SeqCst);

        let holding = Arc::clone(&self.holding);
        let active_id = Arc::clone(&self.active_press_id);
        let fired_id = Arc::clone(&self.long_fired_press_id);
        let cx2 = cx.clone();
        let ctx = ev.context.to_string();
        let elapsed_ms = Arc::clone(&self.elapsed_ms);
        let cancel = Arc::clone(&self.cancel);
        let epoch = Arc::clone(&self.epoch);
        let long_press_ms = self.long_press_ms;

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(long_press_ms));

            if !holding.load(Ordering::SeqCst) {
                return;
            }
            if active_id.load(Ordering::SeqCst) != pid {
                return;
            }

            fired_id.store(pid, Ordering::SeqCst);

            // Reset: stop tick thread and clear elapsed
            cancel.store(true, Ordering::Relaxed);
            epoch.fetch_add(1, Ordering::Relaxed);
            elapsed_ms.store(0, Ordering::Relaxed);
            clear_state(&cx2, &ctx);
            render_time_hhmmss(&cx2, &ctx, 0);
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &incoming::KeyUp) {
        self.holding.store(false, Ordering::SeqCst);

        let pid = self.active_press_id.load(Ordering::SeqCst);
        if self.long_fired_press_id.load(Ordering::SeqCst) == pid {
            // Long press handled the reset
            self.running = false;
            return;
        }

        // Short press: toggle start/stop
        if self.running {
            self.stop_tick();
            self.running = false;
            let elapsed_ms = self.elapsed_ms.load(Ordering::Relaxed);
            save_paused(cx, ev.context, elapsed_ms);
            render_time_hhmmss(cx, ev.context, elapsed_ms / 1000);
        } else {
            let elapsed_ms = self.elapsed_ms.load(Ordering::Relaxed);
            save_running(cx, ev.context, elapsed_ms);
            self.start_tick(cx, ev.context);
            self.running = true;
        }
    }
}

impl StopwatchAction {
    fn stop_tick(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.epoch_seq = self.epoch_seq.wrapping_add(1);
        self.epoch.store(self.epoch_seq, Ordering::Relaxed);
    }

    fn start_tick(&mut self, cx: &Context, ctx_id: &str) {
        self.epoch_seq = self.epoch_seq.wrapping_add(1);
        self.epoch.store(self.epoch_seq, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);

        let my_epoch = self.epoch_seq;
        let elapsed_ms = Arc::clone(&self.elapsed_ms);
        let cancel = Arc::clone(&self.cancel);
        let epoch = Arc::clone(&self.epoch);
        let cx2 = cx.clone();
        let ctx = ctx_id.to_string();

        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(TICK_MS));

                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if epoch.load(Ordering::Relaxed) != my_epoch {
                    break;
                }

                let current = elapsed_ms.load(Ordering::Relaxed);
                let next = current.saturating_add(TICK_MS);
                elapsed_ms.store(next, Ordering::Relaxed);

                // Only update display at whole-second boundaries
                let prev_sec = current / 1000;
                let next_sec = next / 1000;
                if next_sec != prev_sec {
                    render_time_hhmmss(&cx2, &ctx, next_sec);
                }
            }
        });
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

fn parse_settings(v: &Map<String, Value>) -> u64 {
    match v.get("longPressMs") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(500),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(500),
        _ => 500,
    }
}
