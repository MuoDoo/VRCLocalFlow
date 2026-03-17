use anyhow::{Context, Result};
use log::{info, warn};
use std::collections::VecDeque;
use webrtc_vad::{SampleRate, Vad, VadMode};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::protocol::AsrResultData;

const SAMPLE_RATE: usize = 16_000;
const SILENCE_THRESHOLD: f32 = 0.01;

// — VAD parameters —
const VAD_FRAME_MS: usize = 30;
const VAD_FRAME_SAMPLES: usize = SAMPLE_RATE * VAD_FRAME_MS / 1000; // 480 samples
const MAX_SPEECH_SECONDS: f32 = 15.0;
const MAX_SPEECH_SAMPLES: usize = (SAMPLE_RATE as f32 * MAX_SPEECH_SECONDS) as usize;
const END_OF_SPEECH_MS: usize = 700;
const END_OF_SPEECH_SAMPLES: usize = SAMPLE_RATE * END_OF_SPEECH_MS / 1000;
const MIN_SPEECH_MS: usize = 250;
const MIN_SPEECH_SAMPLES: usize = SAMPLE_RATE * MIN_SPEECH_MS / 1000;
const PRE_SPEECH_MS: usize = 300;
const PRE_SPEECH_SAMPLES: usize = SAMPLE_RATE * PRE_SPEECH_MS / 1000;

/// Determine optimal thread count for whisper inference.
fn optimal_thread_count() -> i32 {
    let physical = num_cpus::get_physical();
    let threads = (physical / 2).max(2).min(8);
    threads as i32
}

/// Patterns that whisper outputs for non-speech audio — filter these out.
const JUNK_PATTERNS: &[&str] = &[
    "[BLANK_AUDIO]",
    "[MUSIC]",
    "[APPLAUSE]",
    "[LAUGHTER]",
    "(music)",
    "(applause)",
    "[ Silence ]",
    "[no speech]",
    "[silence]",
    "you",
    "Thank you",
];

fn is_junk(text: &str) -> bool {
    let trimmed = text.trim();
    JUNK_PATTERNS
        .iter()
        .any(|p| trimmed.eq_ignore_ascii_case(p))
}

fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

fn trim_silence(audio: &[f32], threshold: f32) -> &[f32] {
    if audio.is_empty() {
        return audio;
    }
    let window = 160;
    let threshold_sq = threshold * threshold;

    let start = (0..audio.len())
        .step_by(window)
        .find(|&i| {
            let end = (i + window).min(audio.len());
            let chunk = &audio[i..end];
            let energy: f32 = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            energy >= threshold_sq
        })
        .unwrap_or(0);

    let end = (0..audio.len())
        .rev()
        .step_by(window)
        .find(|&i| {
            let chunk_start = i.saturating_sub(window);
            let chunk = &audio[chunk_start..=i.min(audio.len() - 1)];
            let energy: f32 = chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32;
            energy >= threshold_sq
        })
        .map(|i| (i + 1).min(audio.len()))
        .unwrap_or(audio.len());

    if start >= end {
        return audio;
    }

    let pad = SAMPLE_RATE / 20; // 50ms
    let start = start.saturating_sub(pad);
    let end = (end + pad).min(audio.len());
    &audio[start..end]
}

fn f32_frame_to_i16(frame: &[f32]) -> Vec<i16> {
    frame
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

// ---- VAD Processor ----

enum VadState {
    Idle,
    Speaking,
    TrailingSilence,
}

pub struct VadProcessor {
    vad: Vad,
    state: VadState,
    speech_buffer: Vec<f32>,
    ring_buffer: VecDeque<f32>,
    speech_sample_count: usize,
    silence_sample_count: usize,
    frame_buffer: Vec<f32>,
}

impl VadProcessor {
    pub fn new() -> Self {
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        Self {
            vad,
            state: VadState::Idle,
            speech_buffer: Vec::with_capacity(MAX_SPEECH_SAMPLES),
            ring_buffer: VecDeque::with_capacity(PRE_SPEECH_SAMPLES + VAD_FRAME_SAMPLES),
            speech_sample_count: 0,
            silence_sample_count: 0,
            frame_buffer: Vec::with_capacity(VAD_FRAME_SAMPLES * 2),
        }
    }

    pub fn process_chunk(&mut self, chunk: &[f32]) -> Option<Vec<f32>> {
        self.frame_buffer.extend_from_slice(chunk);
        let mut result = None;
        while self.frame_buffer.len() >= VAD_FRAME_SAMPLES {
            let frame: Vec<f32> = self.frame_buffer.drain(..VAD_FRAME_SAMPLES).collect();
            let i16_frame = f32_frame_to_i16(&frame);
            let is_speech = self.vad.is_voice_segment(&i16_frame).unwrap_or(false);
            if let Some(segment) = self.process_frame(&frame, is_speech) {
                result = Some(segment);
            }
        }
        result
    }

    fn process_frame(&mut self, frame: &[f32], is_speech: bool) -> Option<Vec<f32>> {
        match self.state {
            VadState::Idle => {
                if is_speech {
                    self.speech_buffer.clear();
                    self.speech_buffer.extend(self.ring_buffer.iter());
                    self.speech_buffer.extend_from_slice(frame);
                    self.speech_sample_count = frame.len();
                    self.silence_sample_count = 0;
                    self.ring_buffer.clear();
                    self.state = VadState::Speaking;
                } else {
                    self.ring_buffer.extend(frame.iter());
                    while self.ring_buffer.len() > PRE_SPEECH_SAMPLES {
                        self.ring_buffer.pop_front();
                    }
                }
                None
            }
            VadState::Speaking => {
                self.speech_buffer.extend_from_slice(frame);
                if is_speech {
                    self.speech_sample_count += frame.len();
                } else {
                    self.silence_sample_count = frame.len();
                    self.state = VadState::TrailingSilence;
                }
                if self.speech_buffer.len() >= MAX_SPEECH_SAMPLES {
                    return Some(self.emit_and_reset());
                }
                None
            }
            VadState::TrailingSilence => {
                self.speech_buffer.extend_from_slice(frame);
                if is_speech {
                    self.speech_sample_count += frame.len();
                    self.silence_sample_count = 0;
                    self.state = VadState::Speaking;
                } else {
                    self.silence_sample_count += frame.len();
                }
                if self.speech_buffer.len() >= MAX_SPEECH_SAMPLES {
                    return Some(self.emit_and_reset());
                }
                if self.silence_sample_count >= END_OF_SPEECH_SAMPLES {
                    if self.speech_sample_count >= MIN_SPEECH_SAMPLES {
                        return Some(self.emit_and_reset());
                    } else {
                        self.reset();
                        return None;
                    }
                }
                None
            }
        }
    }

    fn emit_and_reset(&mut self) -> Vec<f32> {
        let segment = std::mem::take(&mut self.speech_buffer);
        self.reset();
        segment
    }

    fn reset(&mut self) {
        self.speech_buffer.clear();
        self.ring_buffer.clear();
        self.speech_sample_count = 0;
        self.silence_sample_count = 0;
        self.state = VadState::Idle;
    }

    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.speech_sample_count >= MIN_SPEECH_SAMPLES && !self.speech_buffer.is_empty() {
            Some(self.emit_and_reset())
        } else {
            self.reset();
            None
        }
    }

    pub fn flush_if_speaking(&mut self) -> Option<Vec<f32>> {
        match self.state {
            VadState::Speaking | VadState::TrailingSilence => self.flush(),
            VadState::Idle => None,
        }
    }
}

// ---- ASR Engine ----

pub struct AsrEngine {
    #[allow(dead_code)]
    ctx: WhisperContext,
    state: whisper_rs::WhisperState,
    pub vad: VadProcessor,
    language: String,
    n_threads: i32,
    gpu_available: bool,
}

impl AsrEngine {
    pub fn new(model_path: &str, language: &str) -> Result<Self> {
        info!("Loading whisper model from: {model_path}");

        let sys_info = whisper_rs::SystemInfo::default();
        let sys_info_str = whisper_rs::print_system_info();
        info!("Whisper system info: {sys_info_str}");
        info!(
            "GPU support compiled: cuda={}, blas={}, avx={}, avx2={}, fma={}, f16c={}",
            sys_info.cuda, sys_info.blas, sys_info.avx, sys_info.avx2, sys_info.fma, sys_info.f16c
        );

        let gpu_available = sys_info.cuda;
        if cfg!(feature = "cuda") && !gpu_available {
            warn!("CUDA feature enabled at compile time but CUDA not available at runtime!");
        }

        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu(gpu_available);
        if gpu_available {
            ctx_params.flash_attn(true);
            ctx_params.gpu_device(0);
            info!("Whisper context params: use_gpu=true, flash_attn=true, gpu_device=0");
        } else {
            info!("Whisper context params: use_gpu=false");
        }

        let ctx = WhisperContext::new_with_params(model_path, ctx_params)
            .map_err(|e| anyhow::anyhow!("Failed to load whisper model: {e}"))?;

        let state = ctx
            .create_state()
            .map_err(|e| anyhow::anyhow!("Failed to create whisper state: {e}"))?;

        let n_threads = if gpu_available { 2 } else { optimal_thread_count() };
        info!(
            "Whisper model loaded (threads={n_threads}, gpu={})",
            if gpu_available { "yes" } else { "no" }
        );

        Ok(Self {
            ctx,
            state,
            vad: VadProcessor::new(),
            language: language.to_string(),
            n_threads,
            gpu_available,
        })
    }

    pub fn is_gpu_available(&self) -> bool {
        self.gpu_available
    }

    /// Run inference on a VAD speech segment. Returns None if the segment is junk/silent.
    pub fn transcribe(&mut self, segment: &[f32]) -> Option<AsrResultData> {
        let trimmed = trim_silence(segment, SILENCE_THRESHOLD);
        if trimmed.len() < SAMPLE_RATE / 10 || rms_energy(trimmed) < SILENCE_THRESHOLD {
            return None;
        }

        // Whisper requires >= 1s of audio — pad with silence
        const MIN_WHISPER_SAMPLES: usize = SAMPLE_RATE * 11 / 10;
        let padded: Vec<f32>;
        let audio_for_inference = if trimmed.len() < MIN_WHISPER_SAMPLES {
            padded = {
                let mut buf = trimmed.to_vec();
                buf.resize(MIN_WHISPER_SAMPLES, 0.0);
                buf
            };
            &padded
        } else {
            trimmed
        };

        let audio_duration_ms = (trimmed.len() as f32 / SAMPLE_RATE as f32) * 1000.0;
        let infer_start = std::time::Instant::now();

        match self.recognize(audio_for_inference) {
            Ok(result) => {
                let infer_ms = infer_start.elapsed().as_millis();
                let rtf = infer_ms as f32 / audio_duration_ms;
                info!("Whisper inference: {infer_ms}ms for {audio_duration_ms:.0}ms audio (RTF={rtf:.2})");
                if !result.text.is_empty() && !is_junk(&result.text) {
                    Some(result)
                } else {
                    None
                }
            }
            Err(e) => {
                log::error!("Recognition error: {e:#}");
                None
            }
        }
    }

    fn recognize(&mut self, audio: &[f32]) -> Result<AsrResultData> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        if self.language == "auto" {
            params.set_language(Some("auto"));
        } else {
            params.set_language(Some(&self.language));
        }

        params.set_n_threads(self.n_threads);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_print_special(false);
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);

        self.state
            .full(params, audio)
            .map_err(|e| anyhow::anyhow!("Whisper inference failed: {e}"))?;

        let num_segments = self.state.full_n_segments().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut text = String::new();
        for i in 0..num_segments {
            if let Ok(seg_text) = self.state.full_get_segment_text(i) {
                text.push_str(&seg_text);
            }
        }

        let language = self
            .state
            .full_lang_id_from_state()
            .ok()
            .and_then(|id| whisper_rs::get_lang_str(id).map(|s| s.to_string()))
            .unwrap_or_else(|| "en".to_string());

        Ok(AsrResultData {
            text: text.trim().to_string(),
            language,
        })
    }
}
