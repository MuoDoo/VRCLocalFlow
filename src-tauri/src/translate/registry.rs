use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::download::{is_nllb_downloaded, resolve_models_dir};
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

#[tauri::command]
pub async fn list_translation_models(
    app_handle: AppHandle,
) -> Result<Vec<TranslationModelInfo>, String> {
    let models_dir = resolve_models_dir(&app_handle);
    Ok(vec![TranslationModelInfo {
        id: NLLB_MODEL_DIR.to_string(),
        name: "NLLB-200 (all language pairs)".to_string(),
        size_mb: 600,
        downloaded: is_nllb_downloaded(&models_dir),
    }])
}
