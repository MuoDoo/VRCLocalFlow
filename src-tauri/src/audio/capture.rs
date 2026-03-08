use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use rubato::{FftFixedIn, Resampler};
use serde::Serialize;

const TARGET_SAMPLE_RATE: u32 = 16000;

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
}

impl CaptureHandle {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            inner: Mutex::new(None),
            sender,
            receiver,
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
                    let _ = sender.try_send(output[0].clone());
                }
            }
        }
    } else {
        let _ = sender.try_send(mono);
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
                        process_f32_data(data, channels, &sender, &resampler_state);
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("failed to build f32 input stream: {e}"))?
        }
        SampleFormat::I16 => {
            let sender = state.sender.clone();
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
                        process_f32_data(&float_data, channels, &sender, &resampler_state);
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
