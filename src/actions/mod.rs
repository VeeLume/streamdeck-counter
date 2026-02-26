pub mod computed;
pub mod counter;
pub mod stopwatch;
pub mod timer;

pub mod ids {
    use crate::PLUGIN_ID;

    pub const COUNTER: &str = const_format::concatcp!(PLUGIN_ID, ".counter");
    pub const COMPUTED: &str = const_format::concatcp!(PLUGIN_ID, ".computed");
    pub const TIMER: &str = const_format::concatcp!(PLUGIN_ID, ".timer");
    pub const STOPWATCH: &str = const_format::concatcp!(PLUGIN_ID, ".stopwatch");
}
