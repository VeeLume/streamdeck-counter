//! StopwatchAction — thin shell. State and tick live in
//! `crate::adapters::stopwatch::StopwatchAdapter`.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::topics::{STOPWATCH_CTL, StopwatchControl};

pub struct StopwatchAction {
    holding: Arc<AtomicBool>,
    press_seq: u64,
    active_press_id: Arc<AtomicU64>,
    long_fired_press_id: Arc<AtomicU64>,

    long_press_ms: u64,
    hello_sent: bool,
}

impl Default for StopwatchAction {
    fn default() -> Self {
        Self {
            holding: Arc::new(AtomicBool::new(false)),
            press_seq: 0,
            active_press_id: Arc::new(AtomicU64::new(0)),
            long_fired_press_id: Arc::new(AtomicU64::new(0)),
            long_press_ms: 500,
            hello_sent: false,
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
        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        self.long_press_ms = parse_settings(&ev.settings);
        if !self.hello_sent {
            self.hello_sent = true;
            cx.bus().publish_t(
                STOPWATCH_CTL,
                StopwatchControl::Hello {
                    ctx_id: ev.context.to_string(),
                },
            );
        }
    }

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
                .publish_t(STOPWATCH_CTL, StopwatchControl::Reset { ctx_id: ctx });
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &incoming::KeyUp) {
        self.holding.store(false, Ordering::SeqCst);

        let pid = self.active_press_id.load(Ordering::SeqCst);
        if self.long_fired_press_id.load(Ordering::SeqCst) == pid {
            return;
        }

        cx.bus().publish_t(
            STOPWATCH_CTL,
            StopwatchControl::Toggle {
                ctx_id: ev.context.to_string(),
            },
        );
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
