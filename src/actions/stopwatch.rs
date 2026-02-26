use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_time_hhmmss;

const TICK_MS: u64 = 100;

pub struct StopwatchAction {
    // Shared state between action and tick thread
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

    // Cached settings
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
        cx.sd().get_settings(ctx_id);
        render_time_hhmmss(cx, ctx_id, 0);
    }

    fn did_receive_settings(&mut self, _cx: &Context, ev: &incoming::DidReceiveSettings) {
        self.long_press_ms = parse_settings(&ev.settings);
    }

    fn will_disappear(&mut self, _cx: &Context, _ev: &incoming::WillDisappear) {
        self.stop_tick();
    }

    fn teardown(&mut self, _cx: &Context, _ctx_id: &str) {
        self.stop_tick();
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
            let elapsed_secs = self.elapsed_ms.load(Ordering::Relaxed) / 1000;
            render_time_hhmmss(cx, ev.context, elapsed_secs);
        } else {
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

// ── Settings ────────────────────────────────────────────────────────────────

fn parse_settings(v: &Map<String, Value>) -> u64 {
    match v.get("longPressMs") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(500),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(500),
        _ => 500,
    }
}
