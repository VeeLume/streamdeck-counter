use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_time_mmss;

const TICK_MS: u64 = 100;

// ── Globals persistence ──────────────────────────────────────────────────────

/// State stored in cx.globals()["timers"][ctx_id].
///
/// When paused:  { "remaining_ms": u64 }
/// When running: { "remaining_ms": u64, "anchor_unix_ms": u64 }
///   where remaining = remaining_ms - (now - anchor_unix_ms)  (clamped to 0)
fn load_state(cx: &Context, ctx_id: &str) -> (u64, bool) {
    cx.globals()
        .get("timers")
        .and_then(|v| v.get(ctx_id).cloned())
        .map(|entry| {
            let remaining_base = entry
                .get("remaining_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if let Some(anchor) = entry.get("anchor_unix_ms").and_then(|v| v.as_u64()) {
                let now = unix_now_ms();
                let elapsed = now.saturating_sub(anchor);
                let remaining = remaining_base.saturating_sub(elapsed);
                (remaining, true)
            } else {
                (remaining_base, false)
            }
        })
        .unwrap_or((0, false)) // 0 means no saved state — caller uses duration default
}

fn save_paused(cx: &Context, ctx_id: &str, remaining_ms: u64) {
    cx.globals().with_mut(|m| {
        let map = m
            .entry("timers")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut entry = Map::new();
        entry.insert("remaining_ms".into(), remaining_ms.into());
        map.insert(ctx_id.to_string(), Value::Object(entry));
    });
}

fn save_running(cx: &Context, ctx_id: &str, remaining_ms: u64) {
    let anchor = unix_now_ms();
    cx.globals().with_mut(|m| {
        let map = m
            .entry("timers")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .unwrap();
        let mut entry = Map::new();
        entry.insert("remaining_ms".into(), remaining_ms.into());
        entry.insert("anchor_unix_ms".into(), anchor.into());
        map.insert(ctx_id.to_string(), Value::Object(entry));
    });
}

fn clear_state(cx: &Context, ctx_id: &str) {
    cx.globals().with_mut(|m| {
        if let Some(map) = m.get_mut("timers").and_then(|v| v.as_object_mut()) {
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

pub struct TimerAction {
    remaining_ms: Arc<AtomicU64>,
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

    // Cached settings
    duration_ms: u64,
    long_press_ms: u64,
}

impl Default for TimerAction {
    fn default() -> Self {
        Self {
            remaining_ms: Arc::new(AtomicU64::new(60_000)),
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: Arc::new(AtomicU64::new(0)),
            epoch_seq: 0,
            running: false,

            down_at: None,
            holding: Arc::new(AtomicBool::new(false)),
            press_seq: 0,
            active_press_id: Arc::new(AtomicU64::new(0)),
            long_fired_press_id: Arc::new(AtomicU64::new(0)),

            duration_ms: 60_000,
            long_press_ms: 500,
        }
    }
}

impl ActionStatic for TimerAction {
    const ID: &'static str = super::ids::TIMER;
}

impl Action for TimerAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        // Restore persisted state before requesting settings.
        // If no state is saved yet (first ever init), load_state returns (0, false)
        // and did_receive_settings will set remaining_ms to duration_ms.
        let (remaining_ms, was_running) = load_state(cx, ctx_id);
        if remaining_ms > 0 {
            self.remaining_ms.store(remaining_ms, Ordering::Relaxed);
            self.running = was_running;
            render_time_mmss(cx, ctx_id, remaining_ms / 1000);

            if was_running {
                if remaining_ms == 0 {
                    // Expired while we were gone
                    cx.sd().show_alert(ctx_id);
                    self.running = false;
                    save_paused(cx, ctx_id, 0);
                } else {
                    // Update anchor and restart tick
                    save_running(cx, ctx_id, remaining_ms);
                    self.start_tick(cx, ctx_id);
                }
            }
        }

        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        let (duration_secs, long_press_ms) = parse_settings(&ev.settings);
        let new_duration_ms = duration_secs.saturating_mul(1000);
        self.long_press_ms = long_press_ms;

        if new_duration_ms != self.duration_ms {
            // Duration changed in PI — stop and reset to new duration
            self.stop_tick();
            self.running = false;
            self.duration_ms = new_duration_ms;
            self.remaining_ms.store(self.duration_ms, Ordering::Relaxed);
            clear_state(cx, ev.context);
            render_time_mmss(cx, ev.context, duration_secs);
        } else if self.duration_ms == 60_000 && self.remaining_ms.load(Ordering::Relaxed) == 0 {
            // First-ever init: no persisted state, set to duration
            self.remaining_ms.store(new_duration_ms, Ordering::Relaxed);
            self.duration_ms = new_duration_ms;
            render_time_mmss(cx, ev.context, duration_secs);
        } else if !self.running {
            // Same duration, paused — re-render current remaining
            let remaining_secs = self.remaining_ms.load(Ordering::Relaxed) / 1000;
            render_time_mmss(cx, ev.context, remaining_secs);
        }
        // If running, tick thread is already updating the display
    }

    fn teardown(&mut self, cx: &Context, ctx_id: &str) {
        self.stop_tick();
        let remaining_ms = self.remaining_ms.load(Ordering::Relaxed);
        if self.running {
            save_running(cx, ctx_id, remaining_ms);
        } else {
            save_paused(cx, ctx_id, remaining_ms);
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
        let duration_ms = self.duration_ms;
        let remaining_ms = Arc::clone(&self.remaining_ms);
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

            // Reset timer
            cancel.store(true, Ordering::Relaxed);
            epoch.fetch_add(1, Ordering::Relaxed);
            remaining_ms.store(duration_ms, Ordering::Relaxed);
            clear_state(&cx2, &ctx);
            render_time_mmss(&cx2, &ctx, duration_ms / 1000);
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
            let remaining_ms = self.remaining_ms.load(Ordering::Relaxed);
            save_paused(cx, ev.context, remaining_ms);
            render_time_mmss(cx, ev.context, remaining_ms / 1000);
        } else {
            if self.remaining_ms.load(Ordering::Relaxed) == 0 {
                return;
            }
            let remaining_ms = self.remaining_ms.load(Ordering::Relaxed);
            save_running(cx, ev.context, remaining_ms);
            self.start_tick(cx, ev.context);
            self.running = true;
        }
    }
}

impl TimerAction {
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
        let remaining_ms = Arc::clone(&self.remaining_ms);
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

                let current = remaining_ms.load(Ordering::Relaxed);
                if current == 0 {
                    cx2.sd().show_alert(&ctx);
                    render_time_mmss(&cx2, &ctx, 0);
                    break;
                }

                let next = current.saturating_sub(TICK_MS);
                remaining_ms.store(next, Ordering::Relaxed);

                if next == 0 {
                    cx2.sd().show_alert(&ctx);
                    render_time_mmss(&cx2, &ctx, 0);
                    break;
                }

                let prev_sec = current / 1000;
                let next_sec = next / 1000;
                if next_sec != prev_sec {
                    render_time_mmss(&cx2, &ctx, next_sec);
                }
            }
        });
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Returns (duration_secs, long_press_ms)
fn parse_settings(v: &Map<String, Value>) -> (u64, u64) {
    let duration_secs = match v.get("durationSecs") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(60),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(60),
        _ => 60,
    };
    let long_press_ms = match v.get("longPressMs") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(500),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(500),
        _ => 500,
    };
    (duration_secs.max(1), long_press_ms)
}
