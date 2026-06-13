//! TimerAdjustAction — a "+/-" bump button. On press it publishes an
//! `Adjust` intent on `TIMER_CTL`; the `TimerAdapter` applies the signed
//! delta to every *idle* timer whose name matches `targetTimer`. Running
//! timers ignore it (adjustment is idle-only by design).
//!
//! This action holds no state and never renders a value — it is a pure
//! controller for the timer(s) it targets. Label it in the Stream Deck app
//! (e.g. "+1:00" / "−0:30") to taste.

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_adjust;
use crate::topics::{TIMER_CTL, TimerControl};

#[derive(Default)]
pub struct TimerAdjustAction;

impl ActionStatic for TimerAdjustAction {
    const ID: &'static str = super::ids::TIMER_ADJUST;
}

impl Action for TimerAdjustAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        let (target, delta_secs) = parse_settings(&ev.settings);
        render_adjust(cx, ev.context, delta_secs, &target);
    }

    fn key_down(&mut self, cx: &Context, ev: &incoming::KeyDown) {
        let (target, delta_secs) = parse_settings(&ev.settings);
        if delta_secs == 0 {
            return;
        }
        cx.bus().publish_t(
            TIMER_CTL,
            TimerControl::Adjust {
                target,
                delta_ms: delta_secs.saturating_mul(1000),
            },
        );
    }
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// Returns (target_timer_name, delta_secs). `delta_secs` is signed: positive
/// adds time, negative subtracts.
fn parse_settings(v: &Map<String, Value>) -> (String, i64) {
    let target = v
        .get("targetTimer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let delta_secs = match v.get("deltaSecs") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(60),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(60),
        _ => 60,
    };
    (target, delta_secs)
}
