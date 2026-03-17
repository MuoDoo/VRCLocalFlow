use futures_util::StreamExt;
use log::{info, warn};
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const DEFAULT_MODEL_ID: &str = "base";
const MODELS_SUBDIR: &str = "models";

/// Available whisper models with their metadata.
const WHISPER_MODELS: &[ModelDef] = &[
    ModelDef {
        id: "tiny",
        name: "Tiny",
        filename: "ggml-tiny.bin",
        size_mb: 75,
    },
    ModelDef {
        id: "base",
        name: "Base",
        filename: "ggml-base.bin",
        size_mb: 142,
    },
    ModelDef {
        id: "small",
        name: "Small",
        filename: "ggml-small.bin",
        size_mb: 466,
    },
    ModelDef {
        id: "medium",
        name: "Medium",
        filename: "ggml-medium.bin",
        size_mb: 1500,
    },
    ModelDef {
        id: "large-v3-turbo",
        name: "Large V3 Turbo",
        filename: "ggml-large-v3-turbo.bin",
        size_mb: 1500,
    },
];

struct ModelDef {
    id: &'static str,
    name: &'static str,
    filename: &'static str,
    size_mb: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhisperModelInfo {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub size_mb: u32,
    pub downloaded: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub progress: u32,
}

/// Resolve the models directory, checking resource dir then dev fallbacks.
fn resolve_models_dir(app_handle: &AppHandle) -> PathBuf {
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
    dev_path
}

/// Resolve a model ID (e.g. "small") to its absolute file path.
pub fn resolve_model_path(app_handle: &AppHandle, model_id: &str) -> PathBuf {
    let models_dir = resolve_models_dir(app_handle);
    let filename = WHISPER_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .map(|m| m.filename)
        .unwrap_or_else(|| {
            Box::leak(format!("ggml-{model_id}.bin").into_boxed_str()) as &str
        });
    models_dir.join(filename)
}

#[tauri::command]
pub async fn list_whisper_models(app_handle: AppHandle) -> Result<Vec<WhisperModelInfo>, String> {
    let models_dir = resolve_models_dir(&app_handle);
    let models = WHISPER_MODELS
        .iter()
        .map(|m| {
            let path = models_dir.join(m.filename);
            WhisperModelInfo {
                id: m.id.to_string(),
                name: m.name.to_string(),
                filename: m.filename.to_string(),
                size_mb: m.size_mb,
                downloaded: path.exists(),
            }
        })
        .collect();
    Ok(models)
}

const HF_ORIGIN: &str = "https://huggingface.co";
const HF_MIRROR: &str = "https://hf-mirror.com";
const HF_REPO_PATH: &str = "/ggerganov/whisper.cpp/resolve/main";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn download_endpoints() -> Vec<(String, String)> {
    let mut endpoints = Vec::new();
    if let Ok(endpoint) = std::env::var("HF_ENDPOINT") {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        info!("Using HF_ENDPOINT: {endpoint}");
        endpoints.push((endpoint, HF_REPO_PATH.to_string()));
    }
    endpoints.push((HF_ORIGIN.to_string(), HF_REPO_PATH.to_string()));
    endpoints.push((HF_MIRROR.to_string(), HF_REPO_PATH.to_string()));
    endpoints
}

async fn try_download(
    client: &reqwest::Client,
    base: &str,
    path_prefix: &str,
    filename: &str,
) -> Result<reqwest::Response, String> {
    let url = format!("{base}{path_prefix}/{filename}");
    info!("Trying download from: {url}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("{base}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{base}: HTTP {}", resp.status()));
    }
    Ok(resp)
}

#[tauri::command]
pub async fn download_whisper_model(
    model_id: String,
    app_handle: AppHandle,
) -> Result<(), String> {
    let model_def = WHISPER_MODELS
        .iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown model: {model_id}"))?;

    let models_dir = resolve_models_dir(&app_handle);
    std::fs::create_dir_all(&models_dir).map_err(|e| format!("Failed to create models dir: {e}"))?;

    let dest = models_dir.join(model_def.filename);
    if dest.exists() {
        return Ok(());
    }

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    let endpoints = download_endpoints();
    let mut response: Option<reqwest::Response> = None;
    let mut last_err = String::new();

    for (base, path_prefix) in &endpoints {
        match try_download(&client, base, path_prefix, model_def.filename).await {
            Ok(resp) => {
                info!("Connected to {base}");
                response = Some(resp);
                break;
            }
            Err(e) => {
                warn!("Endpoint failed: {e}");
                last_err = e;
            }
        }
    }

    let response = response.ok_or_else(|| {
        format!("All download endpoints failed. Last error: {last_err}")
    })?;

    let total_size = response.content_length().unwrap_or(0);

    let tmp_dest = std::env::temp_dir().join(format!("rtvt-download-{}", model_def.filename));

    let mut file = tokio::fs::File::create(&tmp_dest)
        .await
        .map_err(|e| format!("Failed to create temp file: {e}"))?;

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_progress: u32 = 0;

    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += chunk.len() as u64;

        let progress = if total_size > 0 {
            ((downloaded as f64 / total_size as f64) * 100.0) as u32
        } else {
            0
        };

        if progress != last_progress {
            last_progress = progress;
            let _ = app_handle.emit(
                "model-download-progress",
                DownloadProgress {
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

    if let Err(_) = tokio::fs::rename(&tmp_dest, &dest).await {
        tokio::fs::copy(&tmp_dest, &dest)
            .await
            .map_err(|e| format!("Failed to copy model file: {e}"))?;
        let _ = tokio::fs::remove_file(&tmp_dest).await;
    }

    info!("Model {} downloaded successfully", model_id);
    let _ = app_handle.emit(
        "model-download-progress",
        DownloadProgress {
            model_id,
            progress: 100,
        },
    );

    Ok(())
}
