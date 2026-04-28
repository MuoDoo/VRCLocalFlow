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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_request(json: &str) -> Request {
        serde_json::from_str(json).expect("valid Request JSON")
    }

    #[test]
    fn deserializes_capabilities_request() {
        let req = parse_request(r#"{"cmd":"capabilities"}"#);
        assert!(matches!(req, Request::Capabilities));
    }

    #[test]
    fn deserializes_init_asr() {
        let req = parse_request(
            r#"{"cmd":"init_asr","model_path":"/tmp/m.bin","language":"en"}"#,
        );
        match req {
            Request::InitAsr { model_path, language } => {
                assert_eq!(model_path, "/tmp/m.bin");
                assert_eq!(language, "en");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserializes_init_translator_with_nllb_codes() {
        let req = parse_request(
            r#"{"cmd":"init_translator","models_root":"/m","source":"eng_Latn","target":"zho_Hans"}"#,
        );
        match req {
            Request::InitTranslator { models_root, source, target } => {
                assert_eq!(models_root, "/m");
                assert_eq!(source, "eng_Latn");
                assert_eq!(target, "zho_Hans");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserializes_asr_with_audio() {
        let req = parse_request(r#"{"cmd":"asr","audio_b64":"AAAA"}"#);
        match req {
            Request::Asr { audio_b64 } => assert_eq!(audio_b64, "AAAA"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn deserializes_shutdown() {
        let req = parse_request(r#"{"cmd":"shutdown"}"#);
        assert!(matches!(req, Request::Shutdown));
    }

    #[test]
    fn rejects_unknown_command() {
        assert!(serde_json::from_str::<Request>(r#"{"cmd":"explode"}"#).is_err());
    }

    #[test]
    fn ok_response_serializes_with_ok_field() {
        let json = serde_json::to_string(&Response::ok()).unwrap();
        assert_eq!(json, r#"{"ok":true}"#);
    }

    #[test]
    fn error_response_serializes_with_error_field() {
        let json = serde_json::to_string(&Response::error("boom")).unwrap();
        assert_eq!(json, r#"{"error":"boom"}"#);
    }

    #[test]
    fn asr_result_response_uses_asr_result_key() {
        let resp = Response::AsrResult {
            asr_result: AsrResultData {
                text: "hi".into(),
                language: "en".into(),
            },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""asr_result""#));
        assert!(json.contains(r#""text":"hi""#));
        assert!(json.contains(r#""language":"en""#));
    }

    #[test]
    fn translate_result_response_uses_translate_result_key() {
        let resp = Response::TranslateResult {
            translate_result: TranslateResultData { text: "你好".into() },
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""translate_result""#));
        assert!(json.contains(r#""text":"你好""#));
    }
}
