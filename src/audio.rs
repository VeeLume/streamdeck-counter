//! Tiny audio helper — emits a short synthesized two-tone beep on demand.
//! Used by the timer adapter to signal expiry audibly.
//!
//! Each beep opens the system default output device fresh, plays, and drops
//! the stream. This costs ~tens of ms per beep but means the plugin always
//! follows the *current* default device — switching audio devices in Windows
//! mid-session works without restarting the plugin. (Holding the stream open
//! at startup, as the previous version did, pinned us to whatever device was
//! default when the plugin launched.)
//!
//! All errors are swallowed (logged at debug/warn only) — audio is non-critical
//! and must never break the timer.

use std::time::Duration;

use rodio::source::SineWave;
use rodio::{DeviceSinkBuilder, Source};
use tracing::{debug, warn};

pub struct Audio;

impl Audio {
    pub fn new() -> Self {
        Self
    }

    /// Play a short two-tone "ding" — non-blocking, returns immediately.
    /// Spawns a detached thread that owns the stream for the duration of
    /// playback, then drops it.
    pub fn play_expiry_beep(&self) {
        std::thread::spawn(|| {
            let stream = match DeviceSinkBuilder::open_default_sink() {
                Ok(s) => s,
                Err(e) => {
                    warn!("audio: could not open default output ({e}); skipping beep");
                    return;
                }
            };

            let mixer = stream.mixer();

            // First tone: 880 Hz (A5)
            let beep1 = SineWave::new(880.0)
                .take_duration(Duration::from_millis(140))
                .fade_in(Duration::from_millis(10))
                .fade_out(Duration::from_millis(40))
                .amplify(0.25);
            mixer.add(beep1);

            // Second tone: 1175 Hz (D6), starting 200ms later
            let beep2 = SineWave::new(1175.0)
                .take_duration(Duration::from_millis(180))
                .fade_in(Duration::from_millis(10))
                .fade_out(Duration::from_millis(60))
                .amplify(0.25)
                .delay(Duration::from_millis(200));
            mixer.add(beep2);

            debug!("audio: beep queued");

            // Hold the stream alive until playback finishes. Total length is
            // 200ms (delay) + 180ms (beep2) = 380ms; pad a bit for fade-out tail.
            std::thread::sleep(Duration::from_millis(450));
        });
    }
}
