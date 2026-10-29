//! The languages a Corpus can be built for.

use strum::{Display, EnumString, VariantArray};

/// A language with a Corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, VariantArray)]
#[strum(ascii_case_insensitive)]
pub enum Language {
    /// Google Books `eng`, Wiktionary `en`.
    #[strum(to_string = "english", serialize = "english", serialize = "eng")]
    English,
    /// Google Books `fre`, Wiktionary `fr`.
    #[strum(to_string = "french", serialize = "french", serialize = "fre")]
    French,
    /// Google Books `ger`, Wiktionary `de`.
    #[strum(to_string = "german", serialize = "german", serialize = "ger")]
    German,
    /// Google Books `spa`, Wiktionary `es`.
    #[strum(to_string = "spanish", serialize = "spanish", serialize = "spa")]
    Spanish,
    /// Google Books `ita`, Wiktionary `it`.
    #[strum(to_string = "italian", serialize = "italian", serialize = "ita")]
    Italian,
    /// Google Books `rus`, Wiktionary `ru`.
    #[strum(to_string = "russian", serialize = "russian", serialize = "rus")]
    Russian,
}

impl Language {
    /// Google Books Ngrams V3 URL slug and ngrams subdirectory name (e.g. "eng", "fre").
    #[must_use]
    pub const fn lang_code(self) -> &'static str {
        match self {
            Self::English => "eng",
            Self::French => "fre",
            Self::German => "ger",
            Self::Spanish => "spa",
            Self::Italian => "ita",
            Self::Russian => "rus",
        }
    }

    /// ISO 639-1 two-character code used by KAIKKI.org Wiktionary extract URLs
    /// and the `lang_code` field in KAIKKI JSONL records (e.g. "en", "de").
    /// Not systematically derivable from `lang_code` — German is "de" not "ge",
    /// Spanish is "es" not "sp".
    #[must_use]
    pub const fn iso_code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Italian => "it",
            Self::Russian => "ru",
        }
    }

    /// URL of the KAIKKI Wiktionary extract for this language.
    ///
    /// English Wiktionary is KAIKKI's primary dataset, published under the
    /// language-name path; other Wiktionary editions are published as
    /// per-ISO-code extracts under `downloads/`. KAIKKI reorganizes these
    /// URLs occasionally (the `downloads/en/` extract was removed in 2026) —
    /// see <https://kaikki.org/dictionary/rawdata.html> for the current layout.
    #[must_use]
    pub fn wiktionary_url(self) -> String {
        if self == Self::English {
            "https://kaikki.org/dictionary/English/kaikki.org-dictionary-English.jsonl.gz"
                .to_string()
        } else {
            let iso = self.iso_code();
            format!("https://kaikki.org/dictionary/downloads/{iso}/{iso}-extract.jsonl.gz")
        }
    }

    /// The largest word length this language has a Supported Corpus for, capped by
    /// `MAX_WORD_LENGTH`.
    #[must_use]
    pub const fn max_supported_length(self) -> usize {
        let largest = match self {
            Self::English | Self::German | Self::Russian => 21,
            Self::French => 20,
            Self::Spanish => 18,
            Self::Italian => 19,
        };
        if largest < crate::MAX_WORD_LENGTH {
            largest
        } else {
            crate::MAX_WORD_LENGTH
        }
    }

    /// Number of Google Books Ngrams V3 shards for this language (confirmed against GCS).
    #[must_use]
    pub const fn shard_count(self) -> u32 {
        match self {
            Self::English => 24,
            Self::French => 6,
            Self::German => 8,
            Self::Spanish => 3,
            Self::Italian | Self::Russian => 2,
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
