use unicode_normalization::UnicodeNormalization;

use crate::error::LexDataError;

/// V3 (2020) line format: `ngram\tyear,match_count,vol[\tyear,match_count,vol]*`
/// All years are on one line; we sum match_count across all year groups.
/// Returns None for multi-word tokens, POS-tagged tokens, and tokens with non-alphabetic chars.
#[optimize(speed)]
pub fn parse_ngram_line(line: &str) -> anyhow::Result<(String, u64)> {
    let (ngram, rest) = line
        .split_once('\t')
        .ok_or_else(|| LexDataError::InvalidParseLine(line.to_string()))?;
    if !is_valid_word(ngram) {
        return Err(LexDataError::InvalidNgram(ngram.to_string()).into());
    }
    let total: u64 = rest
        .split('\t')
        .filter_map(|group| {
            // Each group is "year,match_count,vol_count"
            group.split(',').nth(1)?.parse::<u64>().ok()
        })
        .sum();
    Ok((normalize(ngram), total))
}

/// Rejects multi-word ngrams, POS-tagged tokens (contain `_`),
/// and any token with non-alphabetic characters.
fn is_valid_word(s: &str) -> bool {
    !s.contains([' ', '_']) && s.chars().all(|c| c.is_alphabetic())
}

/// NFC normalization + strip Unicode combining marks (combining class > 0) + lowercase.
///
/// After this transform, `chars().count()` equals the number of game letter slots:
///  - Latin with diacritics: precomposed in NFC (combining class 0) → kept (e.g. é stays é)
///  - Cyrillic: single codepoints, unaffected
///  - Combining vowel marks (if any): stripped
fn normalize(s: &str) -> String {
    s.nfc()
        .filter(|&c| unicode_normalization::char::canonical_combining_class(c) == 0)
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_across_years() {
        let line = "word\t2000,5,3\t2001,10,7";
        let (w, c) = parse_ngram_line(line).unwrap();
        assert_eq!(w, "word");
        assert_eq!(c, 15);
    }

    #[test]
    fn rejects_pos_tagged() {
        assert!(parse_ngram_line("word_NOUN\t2000,5,3").is_err());
    }

    #[test]
    fn rejects_multi_word() {
        assert!(parse_ngram_line("hello world\t2000,5,3").is_err());
    }

    #[test]
    fn rejects_nonalphabetic() {
        assert!(parse_ngram_line("www.example\t2000,5,3").is_err());
    }

    #[test]
    fn lowercases() {
        let (w, _) = parse_ngram_line("Apple\t2000,1,1").unwrap();
        assert_eq!(w, "apple");
    }

    #[test]
    fn preserves_nfc_diacritics() {
        // é (U+00E9, precomposed NFC, combining class 0) is kept — used in French words
        let (w, _) = parse_ngram_line("café\t2000,1,1").unwrap();
        assert_eq!(w, "café");
    }

    #[test]
    fn cyrillic_accepted() {
        // Cyrillic words from Russian shards are valid alphabetic tokens
        let (w, c) = parse_ngram_line("слово\t2000,5,3\t2001,10,7").unwrap();
        assert_eq!(w, "слово");
        assert_eq!(c, 15);
    }
}
