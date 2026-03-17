use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub selected_device: String,
    #[serde(default = "default_source_lang")]
    pub source_lang: String,
    #[serde(default = "default_target_lang")]
    pub target_lang: String,
    #[serde(default)]
    pub tts_enabled: bool,
    #[serde(default = "default_model_path")]
    pub model_path: String,
    #[serde(default)]
    pub tts_output_device: String,
    #[serde(default)]
    pub vrchat_osc_enabled: bool,
    #[serde(default = "default_vrchat_port")]
    pub vrchat_osc_port: u16,
    #[serde(default = "default_backend")]
    pub backend: String,

    /// Legacy field — kept for backward compatibility during deserialization.
    /// Migrated to source_lang/target_lang on load, never written back.
    #[serde(default, skip_serializing)]
    direction: String,
}

fn default_vrchat_port() -> u16 {
    9000
}

fn default_backend() -> String {
    "cpu".to_string()
}

fn default_source_lang() -> String {
    "en".to_string()
}

fn default_target_lang() -> String {
    "zh".to_string()
}

fn default_model_path() -> String {
    "base".to_string()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            selected_device: String::new(),
            source_lang: default_source_lang(),
            target_lang: default_target_lang(),
            tts_enabled: false,
            model_path: default_model_path(),
            tts_output_device: String::new(),
            vrchat_osc_enabled: false,
            vrchat_osc_port: default_vrchat_port(),
            backend: default_backend(),
            direction: String::new(),
        }
    }
}

impl AppSettings {
    /// Migrate legacy `direction` field to `source_lang`/`target_lang`.
    fn migrate(&mut self) {
        if self.direction.is_empty() {
            return;
        }
        // direction was e.g. "en-zh" or "zh-en"
        let parts: Vec<&str> = self.direction.splitn(2, '-').collect();
        if parts.len() == 2 {
            self.source_lang = parts[0].to_string();
            self.target_lang = parts[1].to_string();
        }
        self.direction.clear();
    }
}

fn settings_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| format!("failed to resolve config dir: {e}"))?;
    Ok(dir.join("settings.json"))
}

#[tauri::command]
pub fn load_settings(app_handle: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(&app_handle)?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let data = fs::read_to_string(&path).map_err(|e| format!("read error: {e}"))?;
    let mut settings: AppSettings =
        serde_json::from_str(&data).unwrap_or_else(|_| AppSettings::default());
    settings.migrate();
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(
    app_handle: tauri::AppHandle,
    settings: AppSettings,
) -> Result<(), String> {
    let path = settings_path(&app_handle)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir error: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| format!("json error: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("write error: {e}"))?;
    Ok(())
}
