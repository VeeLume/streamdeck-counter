use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_time_mmss;

// Tick interval for the background thread (100ms for sub-second display accuracy)
const TICK_MS: u64 = 100;

pub struct TimerAction {
    // Shared state between action and tick thread
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
        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        let (duration_secs, long_press_ms) = parse_settings(&ev.settings);
        let new_duration_ms = duration_secs.saturating_mul(1000);
        self.long_press_ms = long_press_ms;

        if new_duration_ms != self.duration_ms {
            // Duration changed — stop and reset to the new duration
            self.stop_tick();
            self.running = false;
            self.duration_ms = new_duration_ms;
            self.remaining_ms.store(self.duration_ms, Ordering::Relaxed);
            render_time_mmss(cx, ev.context, duration_secs);
        } else if !self.running {
            // Same duration, not running — just re-render current remaining time
            let remaining_secs = self.remaining_ms.load(Ordering::Relaxed) / 1000;
            render_time_mmss(cx, ev.context, remaining_secs);
        }
        // If running, the tick thread is already updating the display
    }

    fn will_appear(&mut self, cx: &Context, ev: &incoming::WillAppear) {
        if self.running {
            // Resume the tick thread after a profile/page switch
            self.start_tick(cx, ev.context);
        } else {
            let remaining_secs = self.remaining_ms.load(Ordering::Relaxed) / 1000;
            render_time_mmss(cx, ev.context, remaining_secs);
        }
    }

    fn will_disappear(&mut self, _cx: &Context, _ev: &incoming::WillDisappear) {
        // Stop the tick thread but keep self.running = true so will_appear can restart it
        self.stop_tick();
    }

    fn teardown(&mut self, _cx: &Context, _ctx_id: &str) {
        self.stop_tick();
    }

    fn key_down(&mut self, cx: &Context, ev: &incoming::KeyDown) {
        // Start long-press timer
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
            // Stop the running tick thread first
            cancel.store(true, Ordering::Relaxed);
            let new_epoch = epoch.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = new_epoch; // tick thread checks epoch itself

            remaining_ms.store(duration_ms, Ordering::Relaxed);
            render_time_mmss(&cx2, &ctx, duration_ms / 1000);
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &incoming::KeyUp) {
        self.holding.store(false, Ordering::SeqCst);

        let pid = self.active_press_id.load(Ordering::SeqCst);
        if self.long_fired_press_id.load(Ordering::SeqCst) == pid {
            // Long press handled the reset — sync our running state
            self.running = false;
            return;
        }

        // Short press: toggle start/stop
        if self.running {
            self.stop_tick();
            self.running = false;
            let remaining_secs = self.remaining_ms.load(Ordering::Relaxed) / 1000;
            render_time_mmss(cx, ev.context, remaining_secs);
        } else {
            // Don't start if already at zero
            if self.remaining_ms.load(Ordering::Relaxed) == 0 {
                return;
            }
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
        // Increment epoch and clear cancel flag for the new thread
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
                    // Already at zero — alert and exit
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

                // Only update display at whole-second boundaries to reduce render calls
                let prev_sec = current / 1000;
                let next_sec = next / 1000;
                if next_sec != prev_sec {
                    render_time_mmss(&cx2, &ctx, next_sec);
                }
            }
        });
    }
}

// ── Settings ────────────────────────────────────────────────────────────────

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
