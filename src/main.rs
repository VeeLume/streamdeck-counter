mod actions;
mod render;
mod state;
mod topics;

use streamdeck_lib::prelude::*;
use tracing::info;

use actions::{
    computed::ComputedAction, counter::CounterAction, stopwatch::StopwatchAction,
    timer::TimerAction,
};

pub const PLUGIN_ID: &str = "icu.veelume.counter";

fn main() -> anyhow::Result<()> {
    let _guard = init(PLUGIN_ID);
    info!("Starting V's Counter Stream Deck plugin");

    let plugin = Plugin::new()
        .add_action(ActionFactory::default_of::<CounterAction>())
        .add_action(ActionFactory::default_of::<ComputedAction>())
        .add_action(ActionFactory::default_of::<TimerAction>())
        .add_action(ActionFactory::default_of::<StopwatchAction>());

    run_plugin(plugin)
}
