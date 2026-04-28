use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use rubato::{FftFixedIn, Resampler};
use serde::Serialize;

const TARGET_SAMPLE_RATE: u32 = 16000;

/// Audio buffer capacity in chunks. Each chunk is one resampler output (~1024 samples ≈ 64 ms
/// at 16 kHz), so 100 ≈ 6 s of head-room before we start dropping oldest data.
const AUDIO_CHANNEL_CAPACITY: usize = 100;

/// Throttle for the "audio dropped" warning (one log per N drops).
const DROP_WARN_EVERY: u64 = 50;

#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub name: String,
}

/// Wrapper to make cpal::Stream usable in Tauri managed state.
/// cpal::Stream is safe to drop from any thread; the !Send marker is overly conservative.
#[allow(dead_code)]
struct SendStream(Stream);
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

/// State held while capture is active.
struct CaptureState {
    _stream: SendStream,
}

/// Managed Tauri state for the capture module.
#[allow(private_interfaces)]
pub struct CaptureHandle {
    pub(crate) inner: Mutex<Option<CaptureState>>,
    pub sender: Sender<Vec<f32>>,
    #[allow(dead_code)]
    pub receiver: Receiver<Vec<f32>>,
    /// Cumulative count of dropped audio chunks (engine couldn't keep up).
    drops: std::sync::Arc<AtomicU64>,
}

impl CaptureHandle {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(AUDIO_CHANNEL_CAPACITY);
        Self {
            inner: Mutex::new(None),
            sender,
            receiver,
            drops: std::sync::Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Send a chunk on a bounded channel, dropping the oldest chunk if full.
/// Drops are counted and logged at most once per `DROP_WARN_EVERY`.
fn send_drop_oldest(
    sender: &Sender<Vec<f32>>,
    receiver_for_drain: &Receiver<Vec<f32>>,
    drops: &std::sync::Arc<AtomicU64>,
    chunk: Vec<f32>,
) {
    match sender.try_send(chunk) {
        Ok(()) => {}
        Err(TrySendError::Full(chunk)) => {
            // Drop the oldest queued chunk to make room. We drain one and retry once.
            let _ = receiver_for_drain.try_recv();
            let total = drops.fetch_add(1, Ordering::Relaxed) + 1;
            if total % DROP_WARN_EVERY == 0 {
                log::warn!(
                    "audio: engine cannot keep up — dropped {total} chunks (oldest discarded)"
                );
            }
            let _ = sender.try_send(chunk);
        }
        Err(TrySendError::Disconnected(_)) => {
            // Receiver gone; nothing to do.
        }
    }
}

fn find_device_by_name(name: &str) -> Result<Device> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .context("failed to enumerate input devices")?;
    for device in devices {
        if let Ok(n) = device.name() {
            if n == name {
                return Ok(device);
            }
        }
    }
    Err(anyhow!("input device not found: {}", name))
}

/// Pick a supported input config, preferring f32 samples.
fn pick_input_config(device: &Device) -> Result<(StreamConfig, SampleFormat, u32)> {
    let mut configs: Vec<_> = device
        .supported_input_configs()
        .context("failed to query supported input configs")?
        .collect();

    // Sort so F32 comes first.
    configs.sort_by_key(|c| match c.sample_format() {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        _ => 2,
    });

    let supported = configs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("no supported input configs"))?;

    let sample_format = supported.sample_format();

    // Try to pick a config at 16kHz if supported, otherwise use the max.
    let config = if supported.min_sample_rate().0 <= TARGET_SAMPLE_RATE
        && supported.max_sample_rate().0 >= TARGET_SAMPLE_RATE
    {
        supported
            .with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE))
            .into()
    } else {
        supported.with_max_sample_rate().into()
    };

    let stream_config: StreamConfig = config;
    let device_rate = stream_config.sample_rate.0;

    Ok((stream_config, sample_format, device_rate))
}

/// Convert interleaved multi-channel samples to mono by averaging channels.
fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Build a resampler that converts `from_rate` -> 16 kHz.
fn build_resampler(from_rate: u32, chunk_size: usize) -> Result<FftFixedIn<f32>> {
    let resampler = FftFixedIn::<f32>::new(
        from_rate as usize,
        TARGET_SAMPLE_RATE as usize,
        chunk_size,
        1, // sub-chunks
        1, // single channel after mono mixdown
    )
    .context("failed to create resampler")?;
    Ok(resampler)
}

/// Process audio data: convert to mono, resample if needed, and send to channel.
fn process_f32_data(
    data: &[f32],
    channels: u16,
    sender: &Sender<Vec<f32>>,
    receiver_for_drain: &Receiver<Vec<f32>>,
    drops: &std::sync::Arc<AtomicU64>,
    resampler_state: &Option<Mutex<(FftFixedIn<f32>, Vec<f32>)>>,
) {
    let mono = to_mono(data, channels);

    if let Some(ref rs) = resampler_state {
        let mut lock = rs.lock().unwrap();
        let (ref mut resampler, ref mut buf) = *lock;
        buf.extend_from_slice(&mono);

        let chunk = resampler.input_frames_next();
        while buf.len() >= chunk {
            let input_chunk: Vec<f32> = buf.drain(..chunk).collect();
            if let Ok(output) = resampler.process(&[&input_chunk], None) {
                if !output.is_empty() {
                    send_drop_oldest(sender, receiver_for_drain, drops, output[0].clone());
                }
            }
        }
    } else {
        send_drop_oldest(sender, receiver_for_drain, drops, mono);
    }
}

// ── Tauri commands ──────────────────────────────────────────────────────

#[tauri::command]
pub fn list_audio_devices() -> std::result::Result<Vec<AudioDevice>, String> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("failed to enumerate input devices: {e}"))?;
    let mut result = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            result.push(AudioDevice { name });
        }
    }
    Ok(result)
}

#[tauri::command]
pub fn start_capture(
    device_name: String,
    state: tauri::State<'_, CaptureHandle>,
) -> std::result::Result<(), String> {
    let mut guard = state.inner.lock().map_err(|e| format!("lock error: {e}"))?;
    if guard.is_some() {
        return Err("capture already running".into());
    }

    state.drops.store(0, Ordering::Relaxed);
    // Drain any stale samples from a previous session.
    while state.receiver.try_recv().is_ok() {}

    let device = find_device_by_name(&device_name).map_err(|e| e.to_string())?;
    let (config, sample_format, device_rate) =
        pick_input_config(&device).map_err(|e| e.to_string())?;

    let channels = config.channels;
    let needs_resample = device_rate != TARGET_SAMPLE_RATE;
    let resample_chunk: usize = 1024;

    let err_fn = |err: cpal::StreamError| {
        log::error!("audio stream error: {err}");
    };

    let stream = match sample_format {
        SampleFormat::F32 => {
            let sender = state.sender.clone();
            let drain = state.receiver.clone();
            let drops = state.drops.clone();
            let resampler_state: Option<Mutex<(FftFixedIn<f32>, Vec<f32>)>> = if needs_resample {
                let r = build_resampler(device_rate, resample_chunk).map_err(|e| e.to_string())?;
                Some(Mutex::new((r, Vec::with_capacity(resample_chunk * 2))))
            } else {
                None
            };

            device
                .build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        process_f32_data(data, channels, &sender, &drain, &drops, &resampler_state);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build f32 input stream: {e}"))?
        }
        SampleFormat::I16 => {
            let sender = state.sender.clone();
            let drain = state.receiver.clone();
            let drops = state.drops.clone();
            let resampler_state: Option<Mutex<(FftFixedIn<f32>, Vec<f32>)>> = if needs_resample {
                let r = build_resampler(device_rate, resample_chunk).map_err(|e| e.to_string())?;
                Some(Mutex::new((r, Vec::with_capacity(resample_chunk * 2))))
            } else {
                None
            };

            device
                .build_input_stream(
                    &config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        process_f32_data(&float_data, channels, &sender, &drain, &drops, &resampler_state);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build i16 input stream: {e}"))?
        }
        _ => return Err(format!("unsupported sample format: {sample_format:?}")),
    };

    stream
        .play()
        .map_err(|e| format!("failed to play stream: {e}"))?;

    *guard = Some(CaptureState {
        _stream: SendStream(stream),
    });
    log::info!(
        "audio capture started on device: {device_name} (rate={device_rate}, channels={channels}, resample={needs_resample})"
    );
    Ok(())
}

#[tauri::command]
pub fn stop_capture(
    state: tauri::State<'_, CaptureHandle>,
) -> std::result::Result<(), String> {
    let mut guard = state.inner.lock().map_err(|e| format!("lock error: {e}"))?;
    if guard.is_none() {
        return Err("capture is not running".into());
    }
    *guard = None;
    log::info!("audio capture stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_mono_passthrough_single_channel() {
        let stereo = vec![0.1, 0.2, 0.3];
        let mono = to_mono(&stereo, 1);
        assert_eq!(mono, stereo);
    }

    #[test]
    fn to_mono_averages_stereo() {
        // Two frames: (0.0, 1.0) → 0.5; (0.4, 0.8) → 0.6
        let stereo = vec![0.0, 1.0, 0.4, 0.8];
        let mono = to_mono(&stereo, 2);
        assert_eq!(mono, vec![0.5, 0.6]);
    }

    #[test]
    fn to_mono_averages_5_1() {
        let frame: Vec<f32> = (0..6).map(|i| i as f32).collect(); // 0..5
        let mono = to_mono(&frame, 6);
        // (0+1+2+3+4+5) / 6 = 2.5
        assert_eq!(mono, vec![2.5]);
    }

    #[test]
    fn resampler_actually_changes_length() {
        // 48 kHz → 16 kHz: output length should average ~1/3 of input length.
        // FftFixedIn needs several chunks of warm-up before it emits, so we feed
        // ~1 second of audio and check the cumulative ratio.
        let mut resampler = build_resampler(48_000, 1024).unwrap();
        let chunk = resampler.input_frames_next();
        let input = vec![0.5_f32; chunk * 50];

        let mut total_out = 0usize;
        let mut total_in = 0usize;
        let mut pos = 0;
        while pos + chunk <= input.len() {
            let in_chunk = &input[pos..pos + chunk];
            let out = resampler.process(&[in_chunk], None).unwrap();
            total_out += out[0].len();
            total_in += chunk;
            pos += chunk;
        }
        assert!(total_out > 0, "resampler never emitted any samples");
        let ratio = total_out as f32 / total_in as f32;
        assert!(
            (ratio - 1.0 / 3.0).abs() < 0.05,
            "ratio {ratio} not ~0.333 (out={total_out}, in={total_in})",
        );
    }

    #[test]
    fn send_drop_oldest_drops_when_full() {
        let (tx, rx) = crossbeam_channel::bounded::<Vec<f32>>(2);
        let drops = std::sync::Arc::new(AtomicU64::new(0));

        send_drop_oldest(&tx, &rx, &drops, vec![1.0]);
        send_drop_oldest(&tx, &rx, &drops, vec![2.0]);
        // Channel full — next send must drop the oldest (vec![1.0]).
        send_drop_oldest(&tx, &rx, &drops, vec![3.0]);

        assert_eq!(drops.load(Ordering::Relaxed), 1);

        let received: Vec<Vec<f32>> = rx.try_iter().collect();
        // The oldest (1.0) was discarded; we keep 2.0 then 3.0.
        assert_eq!(received, vec![vec![2.0], vec![3.0]]);
    }

    #[test]
    fn send_drop_oldest_no_drop_when_room_available() {
        let (tx, rx) = crossbeam_channel::bounded::<Vec<f32>>(4);
        let drops = std::sync::Arc::new(AtomicU64::new(0));

        for i in 0..3 {
            send_drop_oldest(&tx, &rx, &drops, vec![i as f32]);
        }
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert_eq!(rx.try_iter().count(), 3);
    }
}
