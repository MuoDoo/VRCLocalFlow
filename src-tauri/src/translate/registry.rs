use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::NLLB_MODEL_DIR;

/// Supported languages for translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Zh,
    Ja,
}

impl Language {
    /// Whisper language code for ASR.
    pub fn whisper_code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Zh => "zh",
            Language::Ja => "ja",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Language::En => "English",
            Language::Zh => "Chinese",
            Language::Ja => "Japanese",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Language::En),
            "zh" => Some(Language::Zh),
            "ja" => Some(Language::Ja),
            _ => None,
        }
    }

    /// NLLB language code for translation.
    pub fn nllb_code(&self) -> &'static str {
        match self {
            Language::En => "eng_Latn",
            Language::Zh => "zho_Hans",
            Language::Ja => "jpn_Jpan",
        }
    }

    /// TTS voice name for macOS `say` command.
    pub fn tts_voice(&self) -> &'static str {
        match self {
            Language::En => "Samantha",
            Language::Zh => "Tingting",
            Language::Ja => "Kyoko",
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri command: list translation models with download status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct TranslationModelInfo {
    pub id: String,
    pub name: String,
    pub size_mb: u32,
    pub downloaded: bool,
}

fn resolve_models_dir(app_handle: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        let models = resource_dir.join("models");
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

fn is_nllb_model_present(models_dir: &std::path::Path) -> bool {
    let dir = models_dir.join(NLLB_MODEL_DIR);
    dir.join("model.bin").exists() && dir.join("sentencepiece.bpe.model").exists()
}

#[tauri::command]
pub async fn list_translation_models(
    app_handle: AppHandle,
) -> Result<Vec<TranslationModelInfo>, String> {
    let models_dir = resolve_models_dir(&app_handle);
    let result = vec![TranslationModelInfo {
        id: NLLB_MODEL_DIR.to_string(),
        name: "NLLB-200 (all language pairs)".to_string(),
        size_mb: 600,
        downloaded: is_nllb_model_present(&models_dir),
    }];
    Ok(result)
}
