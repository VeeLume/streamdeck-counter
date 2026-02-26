use serde_json::{Map, Value};
use streamdeck_lib::Context;

/// Resolve the counter key for a button.
/// If `counter_id` is empty/whitespace, the button's own context UUID is used
/// (per-key counter). Otherwise the shared counter ID is used.
pub fn counter_key(counter_id: &str, ctx_id: &str) -> String {
    if counter_id.trim().is_empty() {
        ctx_id.to_string()
    } else {
        counter_id.to_string()
    }
}

/// Load the counter, initialising it to `initial` if not yet stored.
/// Returns the current value.
pub fn init_or_load_counter(cx: &Context, key: &str, initial: i64) -> i64 {
    let mut out = initial;
    cx.globals().with_mut(|m| {
        let obj = m
            .entry("counters".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = obj.as_object_mut() {
            out = map.get(key).and_then(|v| v.as_i64()).unwrap_or(initial);
            map.entry(key.to_string()).or_insert(Value::from(out));
        }
    });
    out
}

/// Read the current value of a counter (returns `initial` if not found).
pub fn read_counter(cx: &Context, key: &str, initial: i64) -> i64 {
    cx.globals()
        .get("counters")
        .and_then(|v| v.get(key).and_then(|v| v.as_i64()))
        .unwrap_or(initial)
}

/// Write a counter value to global settings (automatically persisted by SD).
/// Does NOT publish the `COUNTER_CHANGED` topic — callers must do that after
/// calling this function so they can include the value in the notification.
pub fn write_counter(cx: &Context, key: &str, value: i64) {
    cx.globals().with_mut(|m| {
        let obj = m
            .entry("counters".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = obj.as_object_mut() {
            map.insert(key.to_string(), Value::from(value));
        }
    });
}
