use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ct2rs::{Config, TranslationOptions, Translator as Ct2Translator};

use crate::lang::is_nllb_lang_token;

/// Strip the Windows `\\?\` extended-length path prefix.
/// CTranslate2's C++ layer cannot open files via this prefix.
fn strip_unc_prefix(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix("\\\\?\\") {
            return PathBuf::from(stripped);
        }
    }
    path.to_path_buf()
}

/// Custom tokenizer for NLLB that wraps SentencePiece and prepends the source language token.
struct NllbTokenizer {
    spm: sentencepiece::SentencePieceProcessor,
    source_lang: String,
}

impl NllbTokenizer {
    fn new(model_dir: &Path, source_lang: &str) -> Result<Self> {
        let spm_path = model_dir.join("sentencepiece.bpe.model");
        let spm = sentencepiece::SentencePieceProcessor::open(&spm_path)
            .with_context(|| format!("Failed to load SPM from {:?}", spm_path))?;
        Ok(Self {
            spm,
            source_lang: source_lang.to_string(),
        })
    }
}

impl ct2rs::Tokenizer for NllbTokenizer {
    fn encode(&self, input: &str) -> Result<Vec<String>> {
        let pieces = self.spm.encode(input)?;
        let mut tokens: Vec<String> = Vec::with_capacity(pieces.len() + 2);
        tokens.push(self.source_lang.clone());
        for piece in &pieces {
            tokens.push(piece.piece.to_string());
        }
        tokens.push("</s>".to_string());
        Ok(tokens)
    }

    fn decode(&self, tokens: Vec<String>) -> Result<String> {
        let filtered: Vec<String> = tokens
            .into_iter()
            .filter(|t| t != "</s>" && !is_nllb_lang_token(t))
            .collect();
        self.spm
            .decode_pieces(&filtered)
            .map_err(anyhow::Error::new)
    }
}

const NLLB_MODEL_DIR: &str = "nllb-200-distilled-600M";

/// In-process translator using NLLB-200-distilled-600M via CTranslate2.
pub struct Translator {
    translator: Ct2Translator<NllbTokenizer>,
    target_lang: String,
}

impl Translator {
    /// Create a translator for a specific language pair using the NLLB model.
    /// `source_nllb` and `target_nllb` are NLLB language codes such as `eng_Latn`.
    pub fn new(models_root: &Path, source_nllb: &str, target_nllb: &str) -> Result<Self> {
        let model_dir = strip_unc_prefix(&models_root.join(NLLB_MODEL_DIR));
        let config = Config::default();

        let tokenizer = NllbTokenizer::new(&model_dir, source_nllb)?;
        let translator = Ct2Translator::with_tokenizer(&model_dir, tokenizer, &config)
            .with_context(|| format!("Failed to load NLLB model from {:?}", model_dir))?;

        Ok(Self {
            translator,
            target_lang: target_nllb.to_string(),
        })
    }

    /// Translate text from source to target language.
    pub fn translate(&self, text: &str) -> Result<String> {
        let options = TranslationOptions::default();
        let target_prefix = vec![vec![self.target_lang.as_str()]];

        let results = self
            .translator
            .translate_batch_with_target_prefix(&[text], &target_prefix, &options, None)?;

        results
            .into_iter()
            .next()
            .map(|(text, _score)| text)
            .context("Translation returned empty results")
    }
}
