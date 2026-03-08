use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use rubato::{FftFixedIn, Resampler};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct OutputDevice {
    pub name: String,
}

/// Wrapper to make cpal::Stream sendable (same pattern as capture.rs).
struct SendStream(Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

/// Audio player that outputs PCM samples to a cpal output device.
pub struct AudioPlayer {
    sender: Sender<Vec<f32>>,
    _stream: SendStream,
    device_sample_rate: u32,
}

impl AudioPlayer {
    /// Create a new AudioPlayer targeting the named output device.
    pub fn new(device_name: &str) -> Result<Self> {
        let device = find_output_device(device_name)?;
        let (config, _sample_format, device_rate) = pick_output_config(&device)?;

        let (sender, receiver): (Sender<Vec<f32>>, Receiver<Vec<f32>>) =
            crossbeam_channel::unbounded();

        let channels = config.channels as usize;
        let buf: std::sync::Arc<Mutex<Vec<f32>>> =
            std::sync::Arc::new(Mutex::new(Vec::with_capacity(device_rate as usize)));

        // Feed samples from channel into buffer
        let buf_writer = buf.clone();
        std::thread::Builder::new()
            .name("playback-feeder".into())
            .spawn(move || {
                while let Ok(samples) = receiver.recv() {
                    let mut lock = buf_writer.lock().unwrap();
                    lock.extend_from_slice(&samples);
                }
            })
            .context("failed to spawn playback feeder thread")?;

        let buf_reader = buf;
        let stream = device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut lock = buf_reader.lock().unwrap();
                    let frames = data.len() / channels;
                    for i in 0..frames {
                        let sample = if !lock.is_empty() {
                            lock.remove(0)
                        } else {
                            0.0
                        };
                        // Write same sample to all channels
                        for ch in 0..channels {
                            data[i * channels + ch] = sample;
                        }
                    }
                },
                |err| log::error!("output stream error: {err}"),
                None,
            )
            .context("failed to build output stream")?;

        stream.play().context("failed to play output stream")?;

        Ok(Self {
            sender,
            _stream: SendStream(stream),
            device_sample_rate: device_rate,
        })
    }

    /// Send PCM samples (mono, f32) at the given sample rate to the output device.
    /// Resamples if the source rate differs from the device rate.
    pub fn play(&self, samples: Vec<f32>, source_sample_rate: u32) -> Result<()> {
        let output = if source_sample_rate != self.device_sample_rate {
            resample(&samples, source_sample_rate, self.device_sample_rate)?
        } else {
            samples
        };
        self.sender
            .send(output)
            .map_err(|_| anyhow!("playback channel closed"))?;
        Ok(())
    }
}

/// Resample mono f32 audio from one rate to another using rubato.
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let chunk_size = 1024.min(samples.len());
    let mut resampler = FftFixedIn::<f32>::new(
        from_rate as usize,
        to_rate as usize,
        chunk_size,
        1, // sub-chunks
        1, // mono
    )
    .context("failed to create resampler")?;

    let mut output = Vec::with_capacity(
        (samples.len() as f64 * to_rate as f64 / from_rate as f64) as usize + 1024,
    );

    let frames_needed = resampler.input_frames_next();
    let mut pos = 0;

    while pos + frames_needed <= samples.len() {
        let chunk = &samples[pos..pos + frames_needed];
        let result = resampler.process(&[chunk], None)?;
        if !result.is_empty() {
            output.extend_from_slice(&result[0]);
        }
        pos += frames_needed;
    }

    // Process remaining samples by zero-padding
    if pos < samples.len() {
        let mut last_chunk = samples[pos..].to_vec();
        last_chunk.resize(frames_needed, 0.0);
        let result = resampler.process(&[&last_chunk], None)?;
        if !result.is_empty() {
            let remaining_output =
                ((samples.len() - pos) as f64 * to_rate as f64 / from_rate as f64) as usize;
            let take = remaining_output.min(result[0].len());
            output.extend_from_slice(&result[0][..take]);
        }
    }

    Ok(output)
}

/// Parse a WAV file's raw bytes and return (f32 samples, sample_rate).
/// Supports PCM f32 little-endian and PCM i16 little-endian formats.
pub fn parse_wav_data(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    // Validate RIFF header
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(anyhow!("not a valid WAV file"));
    }

    // Parse fmt chunk
    let mut pos = 12;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut audio_format = 0u16;
    let mut num_channels = 1u16;

    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;

        if chunk_id == b"fmt " {
            if chunk_size < 16 || pos + 8 + chunk_size > bytes.len() {
                return Err(anyhow!("invalid fmt chunk"));
            }
            let fmt = &bytes[pos + 8..pos + 8 + chunk_size];
            audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
            num_channels = u16::from_le_bytes([fmt[2], fmt[3]]);
            sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);
        }

        if chunk_id == b"data" {
            let data_start = pos + 8;
            let data_end = (data_start + chunk_size).min(bytes.len());
            let raw = &bytes[data_start..data_end];

            let interleaved: Vec<f32> = match (audio_format, bits_per_sample) {
                // PCM float 32-bit (format 3 = IEEE float)
                (3, 32) => raw
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect(),
                // PCM int 16-bit (format 1 = PCM)
                (1, 16) => raw
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
                    .collect(),
                _ => {
                    return Err(anyhow!(
                        "unsupported WAV format: audio_format={audio_format}, bits={bits_per_sample}"
                    ));
                }
            };

            // Mix to mono if needed
            let mono = if num_channels > 1 {
                let ch = num_channels as usize;
                interleaved
                    .chunks_exact(ch)
                    .map(|frame| frame.iter().sum::<f32>() / ch as f32)
                    .collect()
            } else {
                interleaved
            };

            return Ok((mono, sample_rate));
        }

        // Advance to next chunk (chunks are 2-byte aligned)
        pos += 8 + chunk_size;
        if chunk_size % 2 != 0 {
            pos += 1;
        }
    }

    Err(anyhow!("no data chunk found in WAV file"))
}

fn find_output_device(name: &str) -> Result<Device> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .context("failed to enumerate output devices")?;
    for device in devices {
        if let Ok(n) = device.name() {
            if n == name {
                return Ok(device);
            }
        }
    }
    Err(anyhow!("output device not found: {}", name))
}

fn pick_output_config(device: &Device) -> Result<(StreamConfig, SampleFormat, u32)> {
    let mut configs: Vec<_> = device
        .supported_output_configs()
        .context("failed to query supported output configs")?
        .collect();

    // Prefer F32
    configs.sort_by_key(|c| match c.sample_format() {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        _ => 2,
    });

    let supported = configs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no supported output configs"))?;

    let sample_format = supported.sample_format();

    // Use max sample rate (virtual devices usually support a wide range)
    let config: StreamConfig = supported.with_max_sample_rate().into();
    let device_rate = config.sample_rate.0;

    Ok((config, sample_format, device_rate))
}

// ── Tauri command ──────────────────────────────────────────────────────

#[tauri::command]
pub fn list_output_devices() -> std::result::Result<Vec<OutputDevice>, String> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| format!("failed to enumerate output devices: {e}"))?;
    let mut result = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            result.push(OutputDevice { name });
        }
    }
    Ok(result)
}
