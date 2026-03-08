mod audio;
mod asr;
mod pipeline;
mod settings;
mod translate;
mod tts;
mod vrchat;

use std::sync::Mutex;

use audio::CaptureHandle;
use pipeline::Pipeline;
use pipeline::realtime::PipelineConfig;
use tauri::Manager;

struct PipelineState(Mutex<Pipeline>);

#[tauri::command]
fn start_pipeline(
    config: PipelineConfig,
    pipeline_state: tauri::State<'_, PipelineState>,
    capture_state: tauri::State<'_, CaptureHandle>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Start audio capture first
    audio::capture::start_capture(config.device_id.clone(), capture_state.clone())
        .map_err(|e| e.to_string())?;

    // Get audio receiver
    let receiver = capture_state.receiver.clone();

    // Start the pipeline (ASR → translate → TTS)
    let mut pipeline = pipeline_state
        .0
        .lock()
        .map_err(|e| format!("lock error: {e}"))?;
    if let Err(e) = pipeline.start(config, receiver, app_handle.clone()) {
        // Stop capture if pipeline fails to start
        let _ = audio::capture::stop_capture(capture_state);
        return Err(e.to_string());
    }

    Ok(())
}

#[tauri::command]
fn stop_pipeline(
    pipeline_state: tauri::State<'_, PipelineState>,
    capture_state: tauri::State<'_, CaptureHandle>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    // Stop pipeline first
    let mut pipeline = pipeline_state
        .0
        .lock()
        .map_err(|e| format!("lock error: {e}"))?;
    pipeline.stop(&app_handle);

    // Stop audio capture
    let _ = audio::capture::stop_capture(capture_state);

    Ok(())
}

/// Max log file size before rotation (5 MB).
const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;

fn init_logging() {
    use std::io::Write;
    use std::sync::Mutex;

    // Log dir: %LOCALAPPDATA%/com.rtvt.app/ (Windows) or /tmp/ (fallback)
    let log_dir = std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir())
        .join("com.rtvt.app");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("rtvt.log");
    let log_prev = log_dir.join("rtvt.prev.log");

    // Rotate: if current log exceeds MAX_LOG_SIZE, rename to .prev.log
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > MAX_LOG_SIZE {
            let _ = std::fs::rename(&log_path, &log_prev);
        }
    }

    // Append mode so logs survive across restarts within one rotation cycle
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()
        .map(Mutex::new);

    eprintln!("[RTVT] Log file: {}", log_path.display());

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            let ts = buf.timestamp_millis();
            let line = format!(
                "{} [{}] {} - {}\n",
                ts,
                record.level(),
                record.target(),
                record.args()
            );
            // Write to stderr (default)
            let _ = buf.write_all(line.as_bytes());
            // Write to file
            if let Some(ref file) = log_file {
                if let Ok(mut f) = file.lock() {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.flush();
                }
            }
            Ok(())
        })
        .init();
}

pub fn run() {
    init_logging();

    tauri::Builder::default()
        .manage(CaptureHandle::new())
        .manage(PipelineState(Mutex::new(Pipeline::new())))
        .invoke_handler(tauri::generate_handler![
            audio::capture::list_audio_devices,
            audio::capture::start_capture,
            audio::capture::stop_capture,
            audio::playback::list_output_devices,
            asr::whisper::list_whisper_models,
            asr::whisper::download_whisper_model,
            translate::download::list_translation_download_models,
            translate::download::download_translation_model,
            translate::registry::list_translation_models,
            start_pipeline,
            stop_pipeline,
            settings::load_settings,
            settings::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
