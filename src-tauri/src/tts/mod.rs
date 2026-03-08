#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::audio::playback::AudioPlayer;

static TTS_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_enabled(enabled: bool) {
    TTS_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    TTS_ENABLED.load(Ordering::Relaxed)
}

/// Speak text through the given AudioPlayer (virtual device output).
/// If no player is provided, does nothing (silent).
pub fn speak(text: &str, voice: &str, player: Option<&AudioPlayer>) -> Result<()> {
    if !is_enabled() {
        return Ok(());
    }

    let player = match player {
        Some(p) => p,
        None => return Ok(()),
    };

    #[cfg(target_os = "macos")]
    {
        let (samples, sample_rate) = macos::synthesize(text, voice)?;
        player.play(samples, sample_rate)?;
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: still use legacy speak for now
        let _ = player;
        windows::speak(text, voice)?;
    }

    Ok(())
}
