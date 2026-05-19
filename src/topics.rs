use streamdeck_lib::TopicId;

// ── Counter ────────────────────────────────────────────────────────────────

/// Published whenever a counter value changes.
/// Subscribed by `CounterAction` (for shared counter displays)
/// and `ComputedAction` (to recalculate expressions).
pub const COUNTER_CHANGED: TopicId<CounterChanged> = TopicId::new("counter_changed");

#[derive(Clone, Debug)]
pub struct CounterChanged {
    pub counter_key: String,
    pub value: i64,
}

// ── Timer ──────────────────────────────────────────────────────────────────

/// Control channel from TimerAction → TimerAdapter.
/// The adapter owns timer state and the tick thread; the action just
/// publishes intents.
pub const TIMER_CTL: TopicId<TimerControl> = TopicId::new("timer_ctl");

#[derive(Clone, Debug)]
pub enum TimerControl {
    /// Action mounted (init + first settings). Adapter creates state if absent
    /// or rehydrates from globals; then renders current value.
    Hello { ctx_id: String, duration_ms: u64 },
    /// Settings changed in PI. Adapter resets to new duration only if it changed.
    Reconfigure { ctx_id: String, duration_ms: u64 },
    /// Short press: toggle start/pause.
    Toggle { ctx_id: String },
    /// Long press: reset to duration (paused).
    Reset { ctx_id: String },
}

// ── Stopwatch ──────────────────────────────────────────────────────────────

pub const STOPWATCH_CTL: TopicId<StopwatchControl> = TopicId::new("stopwatch_ctl");

#[derive(Clone, Debug)]
pub enum StopwatchControl {
    Hello { ctx_id: String },
    Toggle { ctx_id: String },
    Reset { ctx_id: String },
}
