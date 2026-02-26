use streamdeck_lib::TopicId;

/// Published whenever a counter value changes.
/// Subscribed by `CounterAction` (for shared counter displays)
/// and `ComputedAction` (to recalculate expressions).
pub const COUNTER_CHANGED: TopicId<CounterChanged> = TopicId::new("counter_changed");

#[derive(Clone, Debug)]
pub struct CounterChanged {
    pub counter_key: String,
    pub value: i64,
}
