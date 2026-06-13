//! TimerAdapter — owns countdown state and the single tick thread for all
//! timer instances. Survives page switches and remains running when no
//! TimerAction is mounted, so a timer that started on page A keeps running
//! while the user is on page B and fires its expiry alert when it returns.
//!
//! Persistence: each entry is mirrored to `cx.globals()["timers"][ctx_id]`
//! on every state transition (start/pause/reset/expire), so a full plugin
//! restart can rehydrate via the saved `anchor_unix_ms`.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::audio::Audio;
use crate::render::{render_expired, render_time_mmss};
use crate::topics::{TIMER_CTL, TimerControl};

const TICK_MS: u64 = 100;

/// Bump clamp bounds. The MM:SS display tops out at 99:59, so cap there.
const MIN_DURATION_MS: u64 = 1_000;
const MAX_DURATION_MS: u64 = 5_999_000; // 99:59

pub struct TimerAdapter;

impl AdapterStatic for TimerAdapter {
    const NAME: &'static str = "timer_adapter";
}

impl Adapter for TimerAdapter {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn policy(&self) -> StartPolicy {
        StartPolicy::Eager
    }
    fn topics(&self) -> &'static [&'static str] {
        &[TIMER_CTL.name]
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
            let state: Mutex<HashMap<String, TimerEntry>> = Mutex::new(HashMap::new());
            let audio = Audio::new();

            loop {
                match rx.recv_timeout(Duration::from_millis(TICK_MS)) {
                    Ok(ev) => {
                        if let Some(ctl) = ev.downcast::<TimerControl>(TIMER_CTL) {
                            handle_ctl(&cx, &state, &audio, ctl);
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                if cancel_for_thread.load(Ordering::Relaxed) {
                    break;
                }
                tick_all(&cx, &state, &audio);
            }
        });

        Ok(AdapterHandle::from_thread(join, move || {
            cancel.store(true, Ordering::Relaxed);
        }))
    }
}

// ── State ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct TimerEntry {
    /// Shared name used to route `Adjust` from bump buttons. May be empty
    /// (then only an empty-target bump matches it).
    name: String,
    /// The PI-configured duration — the target a long-press reset returns to.
    /// Bumps do NOT change this, so reset always discards them.
    configured_duration_ms: u64,
    /// The working duration (PI duration ± any idle bumps). This is what a
    /// fresh start counts down from.
    duration_ms: u64,
    /// Remaining at the last anchor point (or current if paused).
    remaining_ms: u64,
    /// `Some(unix_ms)` = running and remaining_ms is as-of that instant.
    /// `None` = paused; remaining_ms is the live value.
    anchor_unix_ms: Option<u64>,
    /// Last second value rendered, to suppress redundant set_image calls.
    last_rendered_sec: Option<u64>,
    /// True if the timer reached 0 since the last time the action saw it.
    /// Stream Deck drops show_alert for hidden buttons, so we replay it on
    /// the next Hello (when the page comes back).
    expired_unack: bool,
    /// True only when expiry happened during plugin downtime (detected at
    /// load_from_globals time). Audio plays fine off-screen, so the live
    /// tick path does NOT set this — only the missed-while-down case does.
    beep_pending: bool,
}

fn handle_ctl(
    cx: &Context,
    state: &Mutex<HashMap<String, TimerEntry>>,
    audio: &Audio,
    ctl: &TimerControl,
) {
    let mut s = state.lock().unwrap();
    match ctl {
        TimerControl::Hello { ctx_id, name, duration_ms } => {
            // If we already have an in-memory entry, that's authoritative
            // (page switch — never lost it). Otherwise try globals, then
            // fall back to a fresh entry at full duration.
            if !s.contains_key(ctx_id) {
                let entry = load_from_globals(cx, ctx_id)
                    .unwrap_or_else(|| TimerEntry::fresh(*duration_ms));
                s.insert(ctx_id.clone(), entry);
            }
            let entry = s.get_mut(ctx_id).unwrap();
            // Keep the routing name current (PI may have changed it).
            entry.name = name.clone();
            // If the running anchor is stale (we slept), re-anchor.
            if let Some(anchor) = entry.anchor_unix_ms {
                let elapsed = unix_now_ms().saturating_sub(anchor);
                entry.remaining_ms = entry.remaining_ms.saturating_sub(elapsed);
                if entry.remaining_ms == 0 {
                    entry.anchor_unix_ms = None;
                    entry.expired_unack = true;
                } else {
                    entry.anchor_unix_ms = Some(unix_now_ms());
                }
            }
            render_entry(cx, ctx_id, entry);
            // Now that the button is visible again, replay any pending alert.
            if entry.expired_unack {
                cx.sd().show_alert(ctx_id);
                entry.expired_unack = false;
            }
            // Only beep if we actually missed it (plugin was down at expiry).
            // A timer that expired off-screen during normal operation already
            // beeped from the live tick path.
            if entry.beep_pending {
                audio.play_expiry_beep();
                entry.beep_pending = false;
            }
            persist(cx, ctx_id, entry);
        }
        TimerControl::Reconfigure { ctx_id, name, duration_ms } => {
            let entry = s
                .entry(ctx_id.clone())
                .or_insert_with(|| TimerEntry::fresh(*duration_ms));
            entry.name = name.clone();
            // Compare against the configured (PI) duration — `duration_ms` may
            // have drifted from bumps and must not suppress a real PI change.
            if entry.configured_duration_ms != *duration_ms {
                *entry = TimerEntry::fresh(*duration_ms);
                entry.name = name.clone();
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
        TimerControl::Toggle { ctx_id } => {
            if let Some(entry) = s.get_mut(ctx_id) {
                if let Some(anchor) = entry.anchor_unix_ms.take() {
                    // Stop: collapse anchor into remaining
                    let elapsed = unix_now_ms().saturating_sub(anchor);
                    entry.remaining_ms = entry.remaining_ms.saturating_sub(elapsed);
                } else if entry.remaining_ms == 0 {
                    // Expired — short press resets back to full duration
                    entry.remaining_ms = entry.duration_ms;
                    entry.last_rendered_sec = None;
                    entry.expired_unack = false;
                    entry.beep_pending = false;
                } else {
                    // Start
                    entry.anchor_unix_ms = Some(unix_now_ms());
                }
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
        TimerControl::Reset { ctx_id } => {
            if let Some(entry) = s.get_mut(ctx_id) {
                // Discard any bumps: restore the PI-configured duration.
                entry.duration_ms = entry.configured_duration_ms;
                entry.remaining_ms = entry.configured_duration_ms;
                entry.anchor_unix_ms = None;
                entry.last_rendered_sec = None;
                entry.expired_unack = false;
                entry.beep_pending = false;
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
        TimerControl::Adjust { target, delta_ms } => {
            // Apply to every idle timer whose name matches. Running timers
            // ignore adjustments (idle-only by design).
            for (ctx_id, entry) in s.iter_mut() {
                if entry.name != *target || entry.anchor_unix_ms.is_some() {
                    continue;
                }
                let next = (entry.duration_ms as i64)
                    .saturating_add(*delta_ms)
                    .clamp(MIN_DURATION_MS as i64, MAX_DURATION_MS as i64)
                    as u64;
                if next == entry.duration_ms {
                    continue;
                }
                entry.duration_ms = next;
                entry.remaining_ms = next;
                entry.last_rendered_sec = None;
                // A bump on an expired timer revives it to a fresh idle state.
                entry.expired_unack = false;
                entry.beep_pending = false;
                render_entry(cx, ctx_id, entry);
                persist(cx, ctx_id, entry);
            }
        }
    }
}

fn tick_all(cx: &Context, state: &Mutex<HashMap<String, TimerEntry>>, audio: &Audio) {
    let mut s = state.lock().unwrap();
    let now = unix_now_ms();
    let mut transitions: Vec<String> = Vec::new();

    for (ctx_id, entry) in s.iter_mut() {
        let Some(anchor) = entry.anchor_unix_ms else {
            continue;
        };
        let elapsed = now.saturating_sub(anchor);
        if elapsed == 0 {
            continue;
        }
        entry.remaining_ms = entry.remaining_ms.saturating_sub(elapsed);
        entry.anchor_unix_ms = Some(now);

        let cur_sec = entry.remaining_ms / 1000;

        if entry.remaining_ms == 0 {
            entry.anchor_unix_ms = None;
            entry.expired_unack = true;
            cx.sd().show_alert(ctx_id);
            audio.play_expiry_beep();
            render_entry(cx, ctx_id, entry);
            transitions.push(ctx_id.clone());
        } else if Some(cur_sec) != entry.last_rendered_sec {
            render_entry(cx, ctx_id, entry);
        }
    }

    // Persist ones that crossed the expiry boundary (others survive on the
    // anchor — no need to write every 100 ms).
    for id in &transitions {
        if let Some(e) = s.get(id) {
            persist(cx, id, e);
        }
    }
}

impl TimerEntry {
    fn fresh(duration_ms: u64) -> Self {
        Self {
            name: String::new(),
            configured_duration_ms: duration_ms,
            duration_ms,
            remaining_ms: duration_ms,
            anchor_unix_ms: None,
            last_rendered_sec: None,
            expired_unack: false,
            beep_pending: false,
        }
    }
}

fn render_entry(cx: &Context, ctx_id: &str, entry: &mut TimerEntry) {
    // Expired: countdown reached 0 and isn't running. Show a distinct
    // "DONE" state instead of "00:00" — the next short press resets it.
    if entry.remaining_ms == 0 && entry.anchor_unix_ms.is_none() {
        entry.last_rendered_sec = Some(0);
        render_expired(cx, ctx_id, &entry.name, entry.configured_duration_ms / 1000);
        return;
    }
    let secs = entry.remaining_ms / 1000;
    entry.last_rendered_sec = Some(secs);
    render_time_mmss(cx, ctx_id, secs, &entry.name);
}

// ── Globals persistence ──────────────────────────────────────────────────────

fn persist(cx: &Context, ctx_id: &str, entry: &TimerEntry) {
    cx.globals().with_mut(|m| {
        let map = m
            .entry("timers")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut e = Map::new();
        e.insert("duration_ms".into(), entry.duration_ms.into());
        e.insert(
            "configured_duration_ms".into(),
            entry.configured_duration_ms.into(),
        );
        e.insert("remaining_ms".into(), entry.remaining_ms.into());
        if let Some(anchor) = entry.anchor_unix_ms {
            e.insert("anchor_unix_ms".into(), anchor.into());
        }
        if entry.expired_unack {
            e.insert("expired_unack".into(), true.into());
        }
        map.insert(ctx_id.to_string(), Value::Object(e));
    });
}

fn load_from_globals(cx: &Context, ctx_id: &str) -> Option<TimerEntry> {
    let timers = cx.globals().get("timers")?;
    let v = timers.get(ctx_id)?;
    let duration_ms = v.get("duration_ms").and_then(|n| n.as_u64())?;
    // Older saved timers predate the split — fall back to the working duration.
    let configured_duration_ms = v
        .get("configured_duration_ms")
        .and_then(|n| n.as_u64())
        .unwrap_or(duration_ms);
    let mut remaining_ms = v
        .get("remaining_ms")
        .and_then(|n| n.as_u64())
        .unwrap_or(duration_ms);
    let mut anchor = v.get("anchor_unix_ms").and_then(|n| n.as_u64());
    let mut expired_unack = v
        .get("expired_unack")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let mut beep_pending = false;
    if let Some(a) = anchor {
        let elapsed = unix_now_ms().saturating_sub(a);
        remaining_ms = remaining_ms.saturating_sub(elapsed);
        if remaining_ms == 0 {
            anchor = None;
            // Plugin was down when this expired — user never heard the beep,
            // so queue it for the next Hello.
            expired_unack = true;
            beep_pending = true;
        } else {
            anchor = Some(unix_now_ms());
        }
    }
    Some(TimerEntry {
        name: String::new(), // set by the next Hello on mount
        configured_duration_ms,
        duration_ms,
        remaining_ms,
        anchor_unix_ms: anchor,
        last_rendered_sec: None,
        expired_unack,
        beep_pending,
    })
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}
