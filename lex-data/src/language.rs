use strum::{Display, EnumString, VariantNames};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, VariantNames)]
#[strum(ascii_case_insensitive)]
pub enum Language {
    #[strum(to_string = "english", serialize = "english", serialize = "eng")]
    English,
    #[strum(to_string = "french", serialize = "french", serialize = "fre")]
    French,
    #[strum(to_string = "german", serialize = "german", serialize = "ger")]
    German,
    #[strum(to_string = "spanish", serialize = "spanish", serialize = "spa")]
    Spanish,
    #[strum(to_string = "italian", serialize = "italian", serialize = "ita")]
    Italian,
    #[strum(to_string = "russian", serialize = "russian", serialize = "rus")]
    Russian,
}

impl Language {
    /// V3 URL slug and cache directory name (e.g. "eng", "fre").
    pub fn lang_code(self) -> &'static str {
        match self {
            Language::English => "eng",
            Language::French => "fre",
            Language::German => "ger",
            Language::Spanish => "spa",
            Language::Italian => "ita",
            Language::Russian => "rus",
        }
    }

    /// Number of V3 shards for this language (confirmed against GCS).
    pub fn shard_count(self) -> u32 {
        match self {
            Language::English => 24,
            Language::French => 6,
            Language::German => 8,
            Language::Spanish => 3,
            Language::Italian => 2,
            Language::Russian => 2,
        }
    }

    pub fn cache_dir(self) -> &'static str {
        self.lang_code()
    }
}
