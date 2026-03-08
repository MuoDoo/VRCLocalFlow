// macOS TTS using system `say` command

use anyhow::{Context, Result};

use crate::audio::playback;

/// Synthesize text to PCM audio using macOS `say -o`.
/// Returns (mono f32 samples, sample_rate).
pub fn synthesize(text: &str, voice: &str) -> Result<(Vec<f32>, u32)> {
    let tmp_path = format!(
        "/tmp/rtvt_tts_{:?}.wav",
        std::thread::current().id()
    );

    let output = std::process::Command::new("say")
        .args([
            "-v",
            voice,
            "-o",
            &tmp_path,
            "--file-format=WAVE",
            "--data-format=LEF32@22050",
            text,
        ])
        .output()
        .context("failed to run say command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("say command failed: {stderr}");
    }

    let wav_bytes = std::fs::read(&tmp_path).context("failed to read TTS wav file")?;
    let _ = std::fs::remove_file(&tmp_path);

    playback::parse_wav_data(&wav_bytes)
}
