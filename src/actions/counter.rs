use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_number;
use crate::state::{counter_key, init_or_load_counter, read_counter, write_counter};
use crate::topics::{COUNTER_CHANGED, CounterChanged};

pub struct CounterAction {
    // Long-press epoch tracking (same atomic pattern as the old plugin)
    down_at: Option<Instant>,
    holding: Arc<AtomicBool>,
    press_seq: u64,
    active_press_id: Arc<AtomicU64>,
    long_fired_press_id: Arc<AtomicU64>,

    // Cached resolved state for key_up to reference without re-parsing
    counter_key: Option<String>,
    active: Option<CounterSettings>,
}

impl Default for CounterAction {
    fn default() -> Self {
        Self {
            down_at: None,
            holding: Arc::new(AtomicBool::new(false)),
            press_seq: 0,
            active_press_id: Arc::new(AtomicU64::new(0)),
            long_fired_press_id: Arc::new(AtomicU64::new(0)),
            counter_key: None,
            active: None,
        }
    }
}

impl ActionStatic for CounterAction {
    const ID: &'static str = super::ids::COUNTER;
}

impl Action for CounterAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn topics(&self) -> &'static [&'static str] {
        &[COUNTER_CHANGED.name]
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        cx.sd().get_settings(ctx_id);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        let settings = parse_settings(&ev.settings);
        let key = counter_key(&settings.counter_id, ev.context);
        self.counter_key = Some(key.clone());
        let current = init_or_load_counter(cx, &key, settings.initial_value);
        render_number(cx, ev.context, current);
    }

    fn key_down(&mut self, cx: &Context, ev: &incoming::KeyDown) {
        let settings = parse_settings(&ev.settings);
        let key = counter_key(&settings.counter_id, ev.context);
        self.counter_key = Some(key.clone());

        let current = init_or_load_counter(cx, &key, settings.initial_value);
        render_number(cx, ev.context, current);

        // Start a new press epoch
        self.active = Some(settings.clone());
        self.down_at = Some(Instant::now());
        self.holding.store(true, Ordering::SeqCst);

        self.press_seq = self.press_seq.wrapping_add(1);
        let pid = self.press_seq;
        self.active_press_id.store(pid, Ordering::SeqCst);
        self.long_fired_press_id.store(0, Ordering::SeqCst);

        // Spawn the long-press timer thread
        let holding = Arc::clone(&self.holding);
        let active_id = Arc::clone(&self.active_press_id);
        let fired_id = Arc::clone(&self.long_fired_press_id);
        let cx2 = cx.clone();
        let ctx = ev.context.to_string();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(settings.long_press_ms));

            // Only fire if still holding AND this is still the active press
            if !holding.load(Ordering::SeqCst) {
                return;
            }
            if active_id.load(Ordering::SeqCst) != pid {
                return;
            }

            fired_id.store(pid, Ordering::SeqCst);

            let base = read_counter(&cx2, &key, settings.initial_value);
            let next = apply(
                settings.long_action,
                base,
                settings.long_value,
                settings.initial_value,
            );
            if next != base || matches!(settings.long_action, Op::Reset) {
                write_counter(&cx2, &key, next);
                cx2.bus().publish_t(
                    COUNTER_CHANGED,
                    CounterChanged { counter_key: key.clone(), value: next },
                );
                render_number(&cx2, &ctx, next);
            }
        });
    }

    fn key_up(&mut self, cx: &Context, ev: &incoming::KeyUp) {
        self.holding.store(false, Ordering::SeqCst);

        let pid = self.active_press_id.load(Ordering::SeqCst);
        if self.long_fired_press_id.load(Ordering::SeqCst) == pid {
            // Long press already handled this
            return;
        }

        let Some(settings) = self.active.clone() else {
            return;
        };

        let key = counter_key(&settings.counter_id, ev.context);
        let base = read_counter(cx, &key, settings.initial_value);
        let next = apply(
            settings.short_action,
            base,
            settings.short_value,
            settings.initial_value,
        );

        if next == base && !matches!(settings.short_action, Op::Reset) {
            return;
        }

        write_counter(cx, &key, next);
        cx.bus().publish_t(
            COUNTER_CHANGED,
            CounterChanged { counter_key: key.clone(), value: next },
        );
        render_number(cx, ev.context, next);
    }

    fn on_notify(&mut self, cx: &Context, ctx_id: &str, event: &ErasedTopic) {
        if let Some(n) = event.downcast(COUNTER_CHANGED) {
            let my_key = match &self.counter_key {
                Some(k) => k.as_str(),
                None => ctx_id,
            };
            if n.counter_key == my_key {
                render_number(cx, ctx_id, n.value);
            }
        }
    }
}

// ── Settings ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    #[default]
    None,
    Add,
    Subtract,
    Multiply,
    Divide,
    Reset,
    Set,
}

#[derive(Clone, Debug)]
struct CounterSettings {
    counter_id: String,
    initial_value: i64,
    short_action: Op,
    short_value: i64,
    long_action: Op,
    long_value: i64,
    long_press_ms: u64,
}

impl Default for CounterSettings {
    fn default() -> Self {
        Self {
            counter_id: String::new(),
            initial_value: 0,
            short_action: Op::Add,
            short_value: 1,
            long_action: Op::None,
            long_value: 0,
            long_press_ms: 500,
        }
    }
}

fn parse_settings(v: &Map<String, Value>) -> CounterSettings {
    let mut s = CounterSettings::default();
    s.counter_id = get_str(v, "counterId").unwrap_or("").to_string();
    s.initial_value = get_i64(v, "initialValue").unwrap_or(0);
    s.short_action = get_op(v, "shortAction").unwrap_or(Op::Add);
    s.short_value = get_i64(v, "shortValue").unwrap_or(1);
    s.long_action = get_op(v, "longAction").unwrap_or(Op::None);
    s.long_value = get_i64(v, "longValue").unwrap_or(0);
    if let Some(ms) = get_u64(v, "longPressMs") {
        s.long_press_ms = ms;
    }
    s
}

fn get_str<'a>(v: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str()
}

fn get_i64(v: &Map<String, Value>, k: &str) -> Option<i64> {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn get_u64(v: &Map<String, Value>, k: &str) -> Option<u64> {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn get_op(v: &Map<String, Value>, k: &str) -> Option<Op> {
    get_str(v, k).and_then(|s| match s {
        "none" => Some(Op::None),
        "add" => Some(Op::Add),
        "subtract" => Some(Op::Subtract),
        "multiply" => Some(Op::Multiply),
        "divide" => Some(Op::Divide),
        "reset" => Some(Op::Reset),
        "set" => Some(Op::Set),
        _ => None,
    })
}

// ── Math ────────────────────────────────────────────────────────────────────

fn apply(op: Op, base: i64, n: i64, init: i64) -> i64 {
    match op {
        Op::None => base,
        Op::Add => base.saturating_add(n),
        Op::Subtract => base.saturating_sub(n),
        Op::Multiply => base.saturating_mul(n),
        Op::Divide => {
            if n == 0 { base } else { base / n }
        }
        Op::Reset => init,
        Op::Set => n,
    }
}
