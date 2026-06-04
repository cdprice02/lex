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
    let normalized = normalize(ngram);
    if !is_valid_word(&normalized) {
        return Err(LexDataError::InvalidNgram(normalized).into());
    }
    let total: u64 = rest
        .split('\t')
        .filter_map(|group| {
            // Each group is "year,match_count,vol_count"
            group.split(',').nth(1)?.parse::<u64>().ok()
        })
        .sum();
    Ok((normalized, total))
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
mod benches {
    extern crate test;
    use std::hint::black_box;
    use test::Bencher;

    use super::*;

    #[bench]
    fn parse_ngram_line_single(b: &mut Bencher) {
        let line = "apple\t2000,500,10";
        b.iter(|| black_box(parse_ngram_line(line)));
    }

    #[bench]
    fn parse_ngram_line_many_years(b: &mut Bencher) {
        let line = "apple\t2000,1,1\t2001,2,1\t2002,3,1\t2003,4,1\t2004,5,1\
                    \t2005,6,1\t2006,7,1\t2007,8,1\t2008,9,1\t2009,10,1\
                    \t2010,11,1\t2011,12,1\t2012,13,1\t2013,14,1\t2014,15,1\
                    \t2015,16,1\t2016,17,1\t2017,18,1\t2018,19,1\t2019,20,1";
        b.iter(|| black_box(parse_ngram_line(line)));
    }
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

    #[test]
    fn rejects_no_tab() {
        assert!(parse_ngram_line("word").is_err());
    }

    #[test]
    fn combines_malformed_group() {
        // The second year group has no count field; it is skipped, remainder still summed.
        let (_, c) = parse_ngram_line("word\t2000,5,3\tbadgroup\t2001,10,7").unwrap();
        assert_eq!(c, 15);
    }

    #[test]
    fn strips_combining_marks() {
        // NFD input is precomposed to NFC before validation and returned in canonical form
        let (w, _) = parse_ngram_line("e\u{0301}lan\t2000,1,1").unwrap();
        assert_eq!(w, "élan");
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn valid_alphabetic_ngram_always_parses(word in "[a-z]{1,20}") {
            let line = format!("{word}\t2000,100,10");
            prop_assert!(parse_ngram_line(&line).is_ok(), "expected Ok for: {line}");
        }

        #[test]
        fn invalid_word_always_rejected(word in "[a-z]{1,10}", injection in "[_ 0-9]") {
            let line = format!("{word}{injection}\t2000,100,10");
            prop_assert!(parse_ngram_line(&line).is_err(), "expected Err for: {line}");
        }
    }
}
