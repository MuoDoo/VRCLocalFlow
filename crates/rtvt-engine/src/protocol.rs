use serde::{Deserialize, Serialize};

// ---- Requests (stdin → engine) ----

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
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

// ---- Responses (engine → stdout) ----

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Response {
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

#[derive(Debug, Serialize)]
pub struct CapabilitiesInfo {
    pub gpu: String,
    pub vram_mb: u64,
}

#[derive(Debug, Serialize)]
pub struct AsrResultData {
    pub text: String,
    pub language: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateResultData {
    pub text: String,
}

impl Response {
    pub fn ok() -> Self {
        Response::Ok { ok: true }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Response::Error { error: msg.into() }
    }
}
