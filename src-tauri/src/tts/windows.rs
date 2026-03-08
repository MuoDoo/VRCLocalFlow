// Windows TTS using SAPI via windows-rs

use anyhow::Result;

pub fn speak(text: &str, voice: &str) -> Result<()> {
    // Windows SAPI implementation will go here
    let _ = (text, voice);
    log::warn!("Windows TTS not yet implemented");
    Ok(())
}
