//! The built-in Profile registry: which lists a Profile is made of and the checksums they must
//! hash to.

use strum::{Display, EnumString, VariantArray};

use crate::language::Language;
use crate::word::{Word, WordSet};

/// A pinned download and the SHA-256 its normalized form must have.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    /// Where the list is downloaded from.
    pub url: &'static str,
    /// SHA-256 of the download once normalized.
    pub sha256: &'static str,
}

/// One stored list: the sources merged into it and the SHA-256 of the stored (normalized,
/// sorted) file. The two differ when a list merges several sources or has none.
#[derive(Debug, Clone, Copy)]
pub struct ListSpec {
    /// The downloads merged into the list; none for a list derived from others.
    pub sources: &'static [Source],
    /// SHA-256 of the stored file.
    pub sha256: &'static str,
}

/// The language, word length and lists a Profile pairs.
#[derive(Debug, Clone, Copy)]
pub struct ProfileSpec {
    /// Language of the Corpus whose frequencies the lists take.
    pub language: Language,
    /// Length of every word in both lists.
    pub word_length: usize,
    /// The Answer list.
    pub answers: ListSpec,
    /// Guess pool words that are not answers.
    pub guess_only: ListSpec,
}

/// A built-in pairing of an Answer list and a Guess pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, VariantArray)]
#[strum(ascii_case_insensitive)]
pub enum Profile {
    /// The original game: 2,315 answers, 12,972-word Guess pool.
    #[strum(serialize = "wordle")]
    Wordle,
    /// Every word of the original Guess pool is also an answer.
    #[strum(serialize = "wordle-all")]
    WordleAll,
}

/// The pinned Wordle answer list.
const WORDLE_ANSWERS: Source = Source {
    url: "https://gist.githubusercontent.com/cfreshman/a03ef2cba789d8cf00c08f767e0fad7b/raw/50838083dca5359b3cacf1ab677059b40c0e025e/wordle-answers-alphabetical.txt",
    sha256: "f40e47975cf360ff9febe7d49c8e285054d8fa3fb9644cea575922060951d6ee",
};

/// The pinned Wordle allowed-guess list, which excludes the answers.
const WORDLE_GUESS_ONLY: Source = Source {
    url: "https://gist.githubusercontent.com/cfreshman/cdcdf777450c5b5301e439061d29694c/raw/cc443a0688b07ab5947afd3975160975547fc513/wordle-allowed-guesses.txt",
    sha256: "f012ef5ecdbeb94274186abcbbd5388d92bb86fe8ae0635fffcc5e9be4aa3d33",
};

/// SHA-256 of the empty list.
const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

impl Profile {
    /// The registry entry for this Profile.
    pub fn spec(self) -> ProfileSpec {
        match self {
            Self::Wordle => ProfileSpec {
                language: Language::English,
                word_length: 5,
                answers: ListSpec {
                    sources: &[WORDLE_ANSWERS],
                    sha256: WORDLE_ANSWERS.sha256,
                },
                guess_only: ListSpec {
                    sources: &[WORDLE_GUESS_ONLY],
                    sha256: WORDLE_GUESS_ONLY.sha256,
                },
            },
            Self::WordleAll => ProfileSpec {
                language: Language::English,
                word_length: 5,
                answers: ListSpec {
                    sources: &[WORDLE_ANSWERS, WORDLE_GUESS_ONLY],
                    sha256: "feacc549249b5dcb43e2799fbbcda14ebc33d55904fb15d043779104e60c1579",
                },
                guess_only: ListSpec {
                    sources: &[],
                    sha256: EMPTY_SHA256,
                },
            },
        }
    }
}

/// What a loaded Profile contributes to a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileStats {
    /// The Profile's stored name.
    pub name: String,
    /// SHA-256 of the stored Answer list.
    pub answers_sha256: String,
    /// SHA-256 of the stored guess-only list.
    pub guess_only_sha256: String,
    /// Number of words in the Answer list.
    pub answer_count: usize,
    /// Size of the Guess pool (answers plus guess-only words).
    pub guess_count: usize,
    /// The Unseen-word floor: the Corpus's minimum observed count.
    pub floor: u64,
    /// Profile words the Corpus lacks, which took the floor.
    pub floored: usize,
}

/// A loaded Profile: its Guess pool, Answer list and stats.
pub struct ProfileData<const N: usize> {
    /// The Guess pool: Corpus words in Corpus order, then floored words alphabetically.
    pub pool: WordSet<N>,
    /// The Answer list, sorted.
    pub answers: Vec<Word<N>>,
    /// What the Profile contributes to a report.
    pub stats: ProfileStats,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn names_round_trip() {
        for p in [Profile::Wordle, Profile::WordleAll] {
            assert_eq!(Profile::from_str(&p.to_string()).unwrap(), p);
        }
        assert_eq!(Profile::WordleAll.to_string(), "wordle-all");
    }

    #[test]
    fn wordle_all_answers_merge_both_sources_and_keep_no_guess_only() {
        let s = Profile::WordleAll.spec();
        assert_eq!(s.answers.sources.len(), 2);
        assert!(s.guess_only.sources.is_empty());
        assert_eq!(s.language, Language::English);
        assert_eq!(s.word_length, 5);
    }
}
