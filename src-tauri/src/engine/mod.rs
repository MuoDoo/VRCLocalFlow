use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tauri::Manager;

// ---- Backend enum ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineBackend {
    Cpu,
    Cuda,
    Vulkan,
}

impl EngineBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            EngineBackend::Cpu => "cpu",
            EngineBackend::Cuda => "cuda",
            EngineBackend::Vulkan => "vulkan",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            EngineBackend::Cpu => "CPU",
            EngineBackend::Cuda => "CUDA",
            EngineBackend::Vulkan => "Vulkan",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(EngineBackend::Cpu),
            "cuda" => Some(EngineBackend::Cuda),
            "vulkan" => Some(EngineBackend::Vulkan),
            _ => None,
        }
    }
}

// ---- IPC Protocol types (must match crates/rtvt-engine/src/protocol.rs) ----

#[derive(Debug, Serialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum EngineRequest {
    Capabilities,
    InitAsr {
        model_path: String,
        language: String,
    },
    InitTranslator {
        models_root: String,
        source: String,
        target: String,
    },
    Asr {
        audio_b64: String,
    },
    Translate {
        text: String,
    },
    Shutdown,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EngineResponse {
    Ok {
        ok: bool,
    },
    Error {
        error: String,
    },
    Capabilities {
        capabilities: CapabilitiesInfo,
    },
    AsrResult {
        asr_result: AsrResultData,
    },
    TranslateResult {
        translate_result: TranslateResultData,
    },
}

#[derive(Debug, Deserialize)]
pub struct CapabilitiesInfo {
    pub gpu: String,
    pub vram_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AsrResultData {
    pub text: String,
    pub language: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranslateResultData {
    pub text: String,
}

// ---- Engine writer (shared handle for sending commands) ----

#[derive(Clone)]
pub struct EngineWriter {
    stdin: Arc<Mutex<BufWriter<ChildStdin>>>,
}

impl EngineWriter {
    fn send_raw(&self, request: &EngineRequest) -> Result<()> {
        let mut stdin = self.stdin.lock().map_err(|e| anyhow::anyhow!("stdin lock: {e}"))?;
        serde_json::to_writer(&mut *stdin, request)?;
        writeln!(&mut *stdin)?;
        stdin.flush()?;
        Ok(())
    }

    pub fn send_init_asr(&self, model_path: &str, language: &str) -> Result<()> {
        self.send_raw(&EngineRequest::InitAsr {
            model_path: model_path.to_string(),
            language: language.to_string(),
        })
    }

    pub fn send_init_translator(
        &self,
        models_root: &str,
        source: &str,
        target: &str,
    ) -> Result<()> {
        self.send_raw(&EngineRequest::InitTranslator {
            models_root: models_root.to_string(),
            source: source.to_string(),
            target: target.to_string(),
        })
    }

    pub fn send_asr_audio(&self, audio: &[f32]) -> Result<()> {
        use base64::Engine;
        // Encode f32 samples as little-endian bytes, then base64
        let bytes: Vec<u8> = audio
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        self.send_raw(&EngineRequest::Asr { audio_b64: b64 })
    }

    pub fn send_translate(&self, text: &str) -> Result<()> {
        self.send_raw(&EngineRequest::Translate {
            text: text.to_string(),
        })
    }

    pub fn send_shutdown(&self) -> Result<()> {
        self.send_raw(&EngineRequest::Shutdown)
    }
}

// ---- Engine process ----

pub struct EngineProcess {
    child: Child,
    writer: EngineWriter,
    stdout: Option<BufReader<ChildStdout>>,
}

impl EngineProcess {
    /// Spawn an engine sidecar for the given backend.
    pub fn spawn(backend: EngineBackend, app_handle: &tauri::AppHandle) -> Result<Self> {
        let binary_path = resolve_engine_binary(backend, app_handle)?;
        info!("Spawning engine ({}) from: {:?}", backend.as_str(), binary_path);

        let mut cmd = Command::new(&binary_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // For CUDA backend, add CUDA toolkit bin/ to PATH so cublas64_12.dll etc. are found.
        if backend == EngineBackend::Cuda {
            if let Some(path) = cuda_library_path() {
                let current = std::env::var("PATH").unwrap_or_default();
                let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
                cmd.env("PATH", format!("{}{sep}{current}", path.display()));
                info!("Added CUDA library path to engine PATH: {:?}", path);
            } else {
                warn!("CUDA backend selected but CUDA toolkit not found. \
                       Engine may fail to load cublas64_12.dll. \
                       Install CUDA Toolkit or set CUDA_PATH.");
            }
        }

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to spawn engine binary: {:?}. \
                For CUDA backend, ensure CUDA Toolkit is installed.", binary_path))?;

        let stdin = child.stdin.take().context("Failed to get engine stdin")?;
        let stdout = child.stdout.take().context("Failed to get engine stdout")?;
        let stderr = child.stderr.take();

        // Forward engine stderr to the app logger so engine logs aren't lost
        // on Windows GUI apps (which have no visible console).
        if let Some(stderr) = stderr {
            let backend_name = backend.as_str().to_string();
            std::thread::Builder::new()
                .name(format!("engine-stderr-{}", backend.as_str()))
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => info!("[engine-{}] {}", backend_name, line),
                            Err(_) => break,
                        }
                    }
                })
                .ok();
        }

        info!("Engine process started (pid={})", child.id());

        Ok(Self {
            child,
            writer: EngineWriter {
                stdin: Arc::new(Mutex::new(BufWriter::new(stdin))),
            },
            stdout: Some(BufReader::new(stdout)),
        })
    }

    /// Get a cloneable writer handle for sending commands.
    pub fn writer(&self) -> EngineWriter {
        self.writer.clone()
    }

    /// Take the stdout reader (can only be called once).
    pub fn take_stdout(&mut self) -> Option<BufReader<ChildStdout>> {
        self.stdout.take()
    }

    /// Kill the engine process.
    pub fn kill(&mut self) {
        // Try graceful shutdown first
        let _ = self.writer.send_shutdown();
        // Give it a moment to exit
        std::thread::sleep(std::time::Duration::from_millis(100));
        // Force kill if still running
        let _ = self.child.kill();
        let _ = self.child.wait();
        info!("Engine process killed");
    }
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

// ---- Binary path resolution ----

fn resolve_engine_binary(
    backend: EngineBackend,
    app_handle: &tauri::AppHandle,
) -> Result<PathBuf> {
    let exe_ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let base_name = format!("rtvt-engine-{}", backend.as_str());

    // Two possible filenames:
    // 1. With target triple (Tauri externalBin convention): rtvt-engine-cpu-x86_64-pc-windows-msvc.exe
    // 2. Without target triple (plain/manual install): rtvt-engine-cpu.exe
    let triple = env!("TARGET_TRIPLE");
    let sidecar_name = format!("{base_name}-{triple}{exe_ext}");
    let plain_name = format!("{base_name}{exe_ext}");
    let candidates = [&sidecar_name, &plain_name];

    // 1. Same directory as main executable (production / bundled app)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            for name in &candidates {
                let candidate = dir.join(name);
                if candidate.exists() {
                    return Ok(candidate);
                }
            }
        }
    }

    // 2. Tauri resource directory (production / bundled app)
    if let Ok(resource_dir) = app_handle.path().resource_dir() {
        for name in &candidates {
            let candidate = resource_dir.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    // 3. Development mode: cargo target directory (single "rtvt-engine" binary)
    let dev_name = format!("rtvt-engine{exe_ext}");
    for profile in &["debug", "release"] {
        // From workspace root (when running via `cargo tauri dev`, CWD is src-tauri/)
        let candidate = PathBuf::from(format!("../target/{profile}/{dev_name}"));
        if candidate.exists() {
            return Ok(candidate);
        }
        // From workspace root directly
        let candidate = PathBuf::from(format!("target/{profile}/{dev_name}"));
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    anyhow::bail!(
        "Engine binary not found for backend '{}'. \
         Looked for '{}' and '{}'. \
         In dev mode, run: cargo build -p rtvt-engine",
        backend.as_str(),
        sidecar_name,
        plain_name,
    )
}

/// Detect which engine backends have binaries available.
pub fn detect_available_backends(app_handle: &tauri::AppHandle) -> Vec<EngineBackend> {
    let all = [EngineBackend::Cpu, EngineBackend::Cuda, EngineBackend::Vulkan];
    all.into_iter()
        .filter(|b| resolve_engine_binary(*b, app_handle).is_ok())
        .collect()
}

/// Find CUDA toolkit library path for runtime DLL loading.
fn cuda_library_path() -> Option<PathBuf> {
    // 1. CUDA_PATH env var (set by CUDA Toolkit installer)
    if let Ok(cuda_path) = std::env::var("CUDA_PATH") {
        let bin = PathBuf::from(&cuda_path).join("bin");
        if bin.is_dir() {
            return Some(bin);
        }
    }

    // 2. Scan standard Windows install locations
    #[cfg(target_os = "windows")]
    {
        let base = "C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA";
        if let Ok(entries) = std::fs::read_dir(base) {
            // Pick the newest version (reverse-sorted by name)
            let mut versions: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join("bin").is_dir())
                .collect();
            versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
            if let Some(entry) = versions.first() {
                return Some(entry.path().join("bin"));
            }
        }
    }

    None
}

/// Read one response line from the engine stdout.
pub fn read_response(reader: &mut BufReader<ChildStdout>) -> Result<EngineResponse> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.is_empty() {
        anyhow::bail!("Engine process closed stdout (crashed or exited)");
    }
    let response: EngineResponse = serde_json::from_str(line.trim())?;
    Ok(response)
}
