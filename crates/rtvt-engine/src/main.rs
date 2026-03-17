mod asr;
mod lang;
mod protocol;
mod translate;

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::Duration;

use crossbeam_channel::RecvTimeoutError;
use log::{error, info};

use asr::AsrEngine;
use lang::Language;
use protocol::{AsrResultData, CapabilitiesInfo, Request, Response, TranslateResultData};
use translate::Translator;

fn send_response(out: &mut impl Write, response: &Response) {
    if let Ok(json) = serde_json::to_string(response) {
        let _ = writeln!(out, "{json}");
        let _ = out.flush();
    }
}

fn decode_audio_b64(b64: &str) -> anyhow::Result<Vec<f32>> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    // Interpret as little-endian f32 samples
    if bytes.len() % 4 != 0 {
        anyhow::bail!("Audio data length {} is not a multiple of 4", bytes.len());
    }
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    Ok(samples)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Stderr)
        .init();

    info!("rtvt-engine starting");

    // Reader thread: read stdin lines into a channel so we can use recv_timeout
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<String>();
    std::thread::Builder::new()
        .name("stdin-reader".into())
        .spawn(move || {
            let stdin = io::stdin().lock();
            for line in stdin.lines() {
                match line {
                    Ok(line) if !line.is_empty() => {
                        if cmd_tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
            info!("stdin closed");
        })
        .expect("Failed to spawn stdin reader thread");

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let mut asr_engine: Option<AsrEngine> = None;
    let mut translator: Option<Translator> = None;

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let request: Request = match serde_json::from_str(&line) {
                    Ok(r) => r,
                    Err(e) => {
                        send_response(&mut out, &Response::error(format!("Invalid JSON: {e}")));
                        continue;
                    }
                };

                match request {
                    Request::Capabilities => {
                        let gpu = if cfg!(feature = "cuda") {
                            "cuda"
                        } else if cfg!(feature = "vulkan") {
                            "vulkan"
                        } else {
                            "none"
                        };
                        send_response(
                            &mut out,
                            &Response::Capabilities {
                                capabilities: CapabilitiesInfo {
                                    gpu: gpu.to_string(),
                                    vram_mb: 0, // TODO: query actual VRAM
                                },
                            },
                        );
                    }

                    Request::InitAsr {
                        model_path,
                        language,
                    } => {
                        info!("Initializing ASR: model={model_path}, lang={language}");
                        match AsrEngine::new(&model_path, &language) {
                            Ok(engine) => {
                                let gpu_status = if engine.is_gpu_available() {
                                    "GPU"
                                } else {
                                    "CPU"
                                };
                                info!("ASR initialized on {gpu_status}");
                                asr_engine = Some(engine);
                                send_response(&mut out, &Response::ok());
                            }
                            Err(e) => {
                                error!("Failed to initialize ASR: {e:#}");
                                send_response(
                                    &mut out,
                                    &Response::error(format!("init_asr failed: {e}")),
                                );
                            }
                        }
                    }

                    Request::InitTranslator {
                        models_root,
                        source,
                        target,
                    } => {
                        info!("Initializing translator: {source} → {target}");
                        let src_lang = Language::from_code(&source);
                        let tgt_lang = Language::from_code(&target);
                        match (src_lang, tgt_lang) {
                            (Some(src), Some(tgt)) => {
                                match Translator::new(Path::new(&models_root), src, tgt) {
                                    Ok(t) => {
                                        info!("Translator initialized");
                                        translator = Some(t);
                                        send_response(&mut out, &Response::ok());
                                    }
                                    Err(e) => {
                                        error!("Failed to initialize translator: {e:#}");
                                        send_response(
                                            &mut out,
                                            &Response::error(format!(
                                                "init_translator failed: {e}"
                                            )),
                                        );
                                    }
                                }
                            }
                            _ => {
                                send_response(
                                    &mut out,
                                    &Response::error(format!(
                                        "Unsupported language pair: {source} → {target}"
                                    )),
                                );
                            }
                        }
                    }

                    Request::Asr { audio_b64 } => {
                        if let Some(ref mut engine) = asr_engine {
                            match decode_audio_b64(&audio_b64) {
                                Ok(samples) => {
                                    if let Some(segment) = engine.vad.process_chunk(&samples) {
                                        if let Some(result) = engine.transcribe(&segment) {
                                            send_response(
                                                &mut out,
                                                &Response::AsrResult { asr_result: result },
                                            );
                                        }
                                    }
                                    // No response if no segment detected (fire-and-forget)
                                }
                                Err(e) => {
                                    error!("Failed to decode audio: {e}");
                                }
                            }
                        }
                        // Silently ignore if ASR not initialized (audio arrives during init)
                    }

                    Request::Translate { text } => {
                        if let Some(ref translator) = translator {
                            let t_start = std::time::Instant::now();
                            match translator.translate(&text) {
                                Ok(translated) => {
                                    info!(
                                        "Translation ({}ms): {translated}",
                                        t_start.elapsed().as_millis()
                                    );
                                    send_response(
                                        &mut out,
                                        &Response::TranslateResult {
                                            translate_result: TranslateResultData {
                                                text: translated,
                                            },
                                        },
                                    );
                                }
                                Err(e) => {
                                    error!("Translation error: {e:#}");
                                    send_response(
                                        &mut out,
                                        &Response::error(format!("translate failed: {e}")),
                                    );
                                }
                            }
                        } else {
                            send_response(
                                &mut out,
                                &Response::error("Translator not initialized"),
                            );
                        }
                    }

                    Request::Shutdown => {
                        info!("Shutdown requested");
                        send_response(&mut out, &Response::ok());
                        break;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Flush VAD if speaking (timeout-based end-of-speech detection)
                if let Some(ref mut engine) = asr_engine {
                    if let Some(segment) = engine.vad.flush_if_speaking() {
                        if let Some(result) = engine.transcribe(&segment) {
                            send_response(
                                &mut out,
                                &Response::AsrResult { asr_result: result },
                            );
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                info!("Command channel disconnected, exiting");
                break;
            }
        }
    }

    // Flush any remaining audio
    if let Some(ref mut engine) = asr_engine {
        if let Some(segment) = engine.vad.flush() {
            if let Some(result) = engine.transcribe(&segment) {
                send_response(&mut out, &Response::AsrResult { asr_result: result });
            }
        }
    }

    info!("rtvt-engine exiting");
}
