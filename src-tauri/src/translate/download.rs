use futures_util::StreamExt;
use log::{info, warn};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

use super::NLLB_MODEL_DIR;

const HF_ORIGIN: &str = "https://huggingface.co";
const HF_MIRROR: &str = "https://hf-mirror.com";
const HF_NLLB_REPO_PATH: &str = "/JustFrederik/nllb-200-distilled-600M-ct2-int8/resolve/main";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MODELS_SUBDIR: &str = "models";

/// Total expected size of all NLLB model files in bytes (~630 MB).
const NLLB_TOTAL_BYTES: u64 = 630_016_302;

/// Files required for the NLLB model directory.
const NLLB_MODEL_FILES: &[&str] = &[
    "model.bin",
    "config.json",
    "shared_vocabulary.txt",
    "sentencepiece.bpe.model",
];

/// Build the list of download endpoints to try in order.
/// Priority: $HF_ENDPOINT env var > HuggingFace origin > HF mirror.
fn download_endpoints() -> Vec<(String, String)> {
    let mut endpoints = Vec::new();
    if let Ok(endpoint) = std::env::var("HF_ENDPOINT") {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!("Using HF_ENDPOINT: {endpoint}");
        endpoints.push((endpoint, HF_NLLB_REPO_PATH.to_string()));
    }
    endpoints.push((HF_ORIGIN.to_string(), HF_NLLB_REPO_PATH.to_string()));
    endpoints.push((HF_MIRROR.to_string(), HF_NLLB_REPO_PATH.to_string()));
    endpoints
}

#[derive(Debug, Clone, Serialize)]
pub struct NllbDownloadProgress {
    pub model_id: String,
    pub progress: u32,
}

/// Resolve the models directory.
pub(crate) fn resolve_models_dir(app_handle: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let models = resource_dir.join(MODELS_SUBDIR);
        if models.exists() {
            return models;
        }
    }
    let dev_path = PathBuf::from("resources/models");
    if dev_path.exists() {
        return dev_path;
    }
    let src_tauri_path = PathBuf::from("src-tauri/resources/models");
    if src_tauri_path.exists() {
        return src_tauri_path;
    }
    PathBuf::from("resources/models")
}

/// Check if the NLLB model is fully downloaded.
pub(crate) fn is_nllb_downloaded(models_dir: &PathBuf) -> bool {
    let model_dir = models_dir.join(NLLB_MODEL_DIR);
    model_dir.join("model.bin").exists() && model_dir.join("sentencepiece.bpe.model").exists()
}

#[tauri::command]
pub async fn download_translation_model(
    model_id: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    if model_id != NLLB_MODEL_DIR {
        return Err(format!("Unknown model: {model_id}"));
    }

    let models_dir = resolve_models_dir(&app_handle);
    let model_dir = models_dir.join(NLLB_MODEL_DIR);
    std::fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model dir: {e}"))?;

    if is_nllb_downloaded(&models_dir) {
        info!("NLLB model already downloaded");
        let _ = app_handle.emit(
            "translation-download-progress",
            NllbDownloadProgress {
                model_id,
                progress: 100,
            },
        );
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let endpoints = download_endpoints();

    // Track cumulative bytes downloaded across all files for smooth progress
    let mut bytes_downloaded: u64 = 0;
    let mut last_reported_progress: u32 = 0;

    for filename in NLLB_MODEL_FILES {
        let dest = model_dir.join(filename);
        if dest.exists() {
            // Count existing file size toward progress
            if let Ok(meta) = std::fs::metadata(&dest) {
                bytes_downloaded += meta.len();
            }
            let progress = ((bytes_downloaded * 99) / NLLB_TOTAL_BYTES).min(99) as u32;
            if progress > last_reported_progress {
                last_reported_progress = progress;
                let _ = app_handle.emit(
                    "translation-download-progress",
                    NllbDownloadProgress {
                        model_id: model_id.clone(),
                        progress,
                    },
                );
            }
            info!("File {} already exists, skipping", filename);
            continue;
        }

        // Try each endpoint in order
        let mut resp = None;
        let mut last_err = String::new();
        for (base, path) in &endpoints {
            let url = format!("{base}{path}/{filename}");
            info!("Trying download: {url}");
            match client.get(&url).send().await {
                Ok(r) if r.status().is_success() => {
                    info!("Connected to {base}");
                    resp = Some(r);
                    break;
                }
                Ok(r) => {
                    last_err = format!("{base}: HTTP {}", r.status());
                    warn!("Endpoint failed: {last_err}");
                }
                Err(e) => {
                    last_err = format!("{base}: {e}");
                    warn!("Endpoint failed: {last_err}");
                }
            }
        }

        let response = match resp {
            Some(r) => r,
            None => {
                if *filename == "config.json" {
                    warn!("Optional file {filename} not available, skipping");
                    continue;
                }
                return Err(format!(
                    "Failed to download {filename}: all endpoints failed. Last error: {last_err}"
                ));
            }
        };

        let expected_size = response.content_length().unwrap_or(0);
        let tmp_dest = std::env::temp_dir().join(format!("rtvt-nllb-{filename}"));
        // Wipe any stale tmp from prior failed attempt.
        let _ = tokio::fs::remove_file(&tmp_dest).await;
        let mut file = tokio::fs::File::create(&tmp_dest)
            .await
            .map_err(|e| format!("Failed to create temp file: {e}"))?;

        let mut file_bytes: u64 = 0;
        let mut stream = response.bytes_stream();
        use tokio::io::AsyncWriteExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Write error: {e}"))?;

            file_bytes += chunk.len() as u64;
            bytes_downloaded += chunk.len() as u64;
            // Cap at 99% until fully complete
            let progress = ((bytes_downloaded * 99) / NLLB_TOTAL_BYTES).min(99) as u32;
            if progress > last_reported_progress {
                last_reported_progress = progress;
                let _ = app_handle.emit(
                    "translation-download-progress",
                    NllbDownloadProgress {
                        model_id: model_id.clone(),
                        progress,
                    },
                );
            }
        }
        file.flush()
            .await
            .map_err(|e| format!("Flush error: {e}"))?;
        drop(file);

        // Validate size against Content-Length to reject truncated downloads.
        if expected_size > 0 && file_bytes != expected_size {
            let _ = tokio::fs::remove_file(&tmp_dest).await;
            return Err(format!(
                "Download incomplete for {filename}: expected {expected_size} bytes, got {file_bytes}. \
                 Please retry."
            ));
        }
        // model.bin is ~600 MB; the others are still well over 1 KB. Reject anything tiny.
        if file_bytes < 1024 {
            let _ = tokio::fs::remove_file(&tmp_dest).await;
            return Err(format!(
                "Download too small for {filename} ({file_bytes} bytes) — server likely \
                 returned an error page. Please retry or switch HF endpoint."
            ));
        }

        if tokio::fs::rename(&tmp_dest, &dest).await.is_err() {
            tokio::fs::copy(&tmp_dest, &dest)
                .await
                .map_err(|e| format!("Failed to copy {filename}: {e}"))?;
            let _ = tokio::fs::remove_file(&tmp_dest).await;
        }

        info!("Downloaded {filename} for NLLB ({file_bytes} bytes)");
    }

    info!("NLLB model downloaded successfully");
    let _ = app_handle.emit(
        "translation-download-progress",
        NllbDownloadProgress {
            model_id,
            progress: 100,
        },
    );

    Ok(())
}
