//! TimerAction — thin shell. All countdown state and the tick thread live in
//! `crate::adapters::timer::TimerAdapter`. The action just publishes intents
//! (Hello / Reconfigure / Toggle / Reset) on the bus and lets the adapter
//! handle rendering and persistence.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::topics::{TIMER_CTL, TimerControl};

pub struct TimerAction {
    // Long-press tracking — kept here because only actions receive key events
    holding: Arc<AtomicBool>,
    press_seq: u64,
    active_press_id: Arc<AtomicU64>,
    long_fired_press_id: Arc<AtomicU64>,

    // Cached so we can detect duration changes vs first-receive
    duration_ms: u64,
    long_press_ms: u64,
}

impl Default for TimerAction {
    fn default() -> Self {
        Self {
            holding: Arc::new(AtomicBool::new(false)),
            press_seq: 0,
            active_press_id: Arc::new(AtomicU64::new(0)),
            long_fired_press_id: Arc::new(AtomicU64::new(0)),
            duration_ms: 0, // sentinel: 0 = uninitialized, first DidReceiveSettings sends Hello
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

    fn init(&mut self, cx: &Context, _ctx_id: &str) {
        cx.sd().get_settings(_ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        let (duration_secs, long_press_ms, name) = parse_settings(&ev.settings);
        self.long_press_ms = long_press_ms;
        let new_duration_ms = duration_secs.saturating_mul(1000);

        if self.duration_ms == 0 {
            // First settings of this action lifecycle — say hello.
            self.duration_ms = new_duration_ms;
            cx.bus().publish_t(
                TIMER_CTL,
                TimerControl::Hello {
                    ctx_id: ev.context.to_string(),
                    name,
                    duration_ms: new_duration_ms,
                },
            );
        } else {
            // Re-send on any settings change so the adapter picks up a renamed
            // timer even when the duration is unchanged.
            self.duration_ms = new_duration_ms;
            cx.bus().publish_t(
                TIMER_CTL,
                TimerControl::Reconfigure {
                    ctx_id: ev.context.to_string(),
                    name,
                    duration_ms: new_duration_ms,
                },
            );
        }
    }

    // No teardown override — the adapter keeps the timer ticking off-screen.

    fn key_down(&mut self, cx: &Context, ev: &incoming::KeyDown) {
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
            cx2.bus()
                .publish_t(TIMER_CTL, TimerControl::Reset { ctx_id: ctx });
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &incoming::KeyUp) {
        self.holding.store(false, Ordering::SeqCst);

        let pid = self.active_press_id.load(Ordering::SeqCst);
        if self.long_fired_press_id.load(Ordering::SeqCst) == pid {
            // Long-press handled the reset
            return;
        }

        cx.bus().publish_t(
            TIMER_CTL,
            TimerControl::Toggle {
                ctx_id: ev.context.to_string(),
            },
        );
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Returns (duration_secs, long_press_ms, timer_name)
fn parse_settings(v: &Map<String, Value>) -> (u64, u64, String) {
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
    let name = v
        .get("timerName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    (duration_secs.max(1), long_press_ms, name)
}
