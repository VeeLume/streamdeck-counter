mod actions;
mod adapters;
mod audio;
mod render;
mod state;
mod topics;
mod update;

use streamdeck_lib::prelude::*;
use tracing::info;

use actions::{
    computed::ComputedAction, counter::CounterAction, stopwatch::StopwatchAction,
    timer::TimerAction, timer_adjust::TimerAdjustAction,
};
use adapters::{stopwatch::StopwatchAdapter, timer::TimerAdapter};

pub const PLUGIN_ID: &str = "icu.veelume.counter";

fn main() -> anyhow::Result<()> {
    let _guard = init(PLUGIN_ID);
    info!("Starting V's Counter Stream Deck plugin");

    // Check GitHub for a newer release in the background (no Elgato Store
    // auto-update). Never blocks startup; only acts when strictly newer.
    update::spawn_update_check();

    let plugin = Plugin::new()
        .add_action(ActionFactory::default_of::<CounterAction>())
        .add_action(ActionFactory::default_of::<ComputedAction>())
        .add_action(ActionFactory::default_of::<TimerAction>())
        .add_action(ActionFactory::default_of::<TimerAdjustAction>())
        .add_action(ActionFactory::default_of::<StopwatchAction>())
        .add_adapter(TimerAdapter)
        .add_adapter(StopwatchAdapter);

    run_plugin(plugin)
}
