//! Language strings used by the engine sidecar.
//!
//! The engine deliberately does NOT define a `Language` enum or carry display
//! metadata (TTS voice, friendly name, etc.) — that is the host's job
//! (`src-tauri/src/translate/registry.rs`). Here we only need to know what NLLB
//! tokens look like so the tokenizer can recognize and strip them on decode.

/// True if a token looks like an NLLB language tag, e.g. `eng_Latn`.
///
/// NLLB language tags follow the pattern `<3-letter ISO>_<4-letter script>`
/// (8 chars total, underscore at byte index 3).
pub fn is_nllb_lang_token(token: &str) -> bool {
    token.len() == 8 && token.as_bytes().get(3) == Some(&b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_nllb_tags() {
        assert!(is_nllb_lang_token("eng_Latn"));
        assert!(is_nllb_lang_token("zho_Hans"));
        assert!(is_nllb_lang_token("jpn_Jpan"));
    }

    #[test]
    fn rejects_non_tags() {
        assert!(!is_nllb_lang_token("hello"));
        assert!(!is_nllb_lang_token("</s>"));
        assert!(!is_nllb_lang_token("eng-Latn")); // wrong separator
        assert!(!is_nllb_lang_token("en"));
    }
}
