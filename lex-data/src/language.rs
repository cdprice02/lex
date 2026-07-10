use strum::{Display, EnumString, VariantNames};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, VariantNames)]
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
    /// Google Books Ngrams V3 URL slug and ngrams subdirectory name (e.g. "eng", "fre").
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

    /// ISO 639-1 two-character code used by KAIKKI.org Wiktionary extract URLs
    /// and the `lang_code` field in KAIKKI JSONL records (e.g. "en", "de").
    /// Not systematically derivable from lang_code — German is "de" not "ge",
    /// Spanish is "es" not "sp".
    pub fn iso_code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::French => "fr",
            Language::German => "de",
            Language::Spanish => "es",
            Language::Italian => "it",
            Language::Russian => "ru",
        }
    }

    /// URL of the KAIKKI Wiktionary extract for this language.
    ///
    /// English Wiktionary is KAIKKI's primary dataset, published under the
    /// language-name path; other Wiktionary editions are published as
    /// per-ISO-code extracts under `downloads/`. KAIKKI reorganizes these
    /// URLs occasionally (the `downloads/en/` extract was removed in 2026) —
    /// see <https://kaikki.org/dictionary/rawdata.html> for the current layout.
    pub fn wiktionary_url(self) -> String {
        match self {
            Language::English => {
                "https://kaikki.org/dictionary/English/kaikki.org-dictionary-English.jsonl.gz"
                    .to_string()
            }
            _ => {
                let iso = self.iso_code();
                format!("https://kaikki.org/dictionary/downloads/{iso}/{iso}-extract.jsonl.gz")
            }
        }
    }

    /// Number of Google Books Ngrams V3 shards for this language (confirmed against GCS).
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_uses_primary_dataset_url() {
        assert_eq!(
            Language::English.wiktionary_url(),
            "https://kaikki.org/dictionary/English/kaikki.org-dictionary-English.jsonl.gz"
        );
    }

    #[test]
    fn other_editions_use_extract_urls() {
        assert_eq!(
            Language::French.wiktionary_url(),
            "https://kaikki.org/dictionary/downloads/fr/fr-extract.jsonl.gz"
        );
        assert_eq!(
            Language::German.wiktionary_url(),
            "https://kaikki.org/dictionary/downloads/de/de-extract.jsonl.gz"
        );
    }
}
