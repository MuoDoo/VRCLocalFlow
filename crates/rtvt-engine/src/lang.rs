#[derive(Debug, Clone, Copy)]
pub enum Language {
    En,
    Zh,
    Ja,
}

impl Language {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Language::En),
            "zh" => Some(Language::Zh),
            "ja" => Some(Language::Ja),
            _ => None,
        }
    }

    pub fn whisper_code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::Zh => "zh",
            Language::Ja => "ja",
        }
    }

    pub fn nllb_code(&self) -> &'static str {
        match self {
            Language::En => "eng_Latn",
            Language::Zh => "zho_Hans",
            Language::Ja => "jpn_Jpan",
        }
    }
}
