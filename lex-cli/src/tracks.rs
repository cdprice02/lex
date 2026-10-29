//! The benchmark Tracks: one `const` table of every game the project measures.
//!
//! Derived from the supported languages, word lengths, Profiles and guess pools (ADR 0007). A new `Language`
//! variant or a raised `max_supported_length` adds its Tracks without a table edit.

use std::fmt;

use lex_core::{GuessPool, Strategy};
use lex_data::{Language, MIN_WORD_LENGTH, Profile};
use strum::VariantArray;

use crate::config::{Game, RunConfig, Targets};

/// The length whose Corpus Tracks are headline.
const HEADLINE_LENGTH: usize = 5;
/// The Fibonacci lengths whose Corpus Tracks are breadth.
const BREADTH_LENGTHS: [usize; 4] = [3, 8, 13, 21];
/// The breadth length whose Tracks fit the time budget on the full dictionary.
const FULL_BREADTH_LENGTH: usize = 3;
/// Sampled targets on a breadth Track that does not fit the budget in full.
const BREADTH_SAMPLE: usize = 500;
/// Sampled targets on a smoke Track.
const SMOKE_SAMPLE: usize = 100;
/// The seed every sampled Track draws with.
const SAMPLE_SEED: u64 = 0;

/// The Profile Tracks: the Profile, the strategy it is played with, and whether it is judged. All
/// are headline; only the original Wordle Profile is judged (ADR 0007). The last is the original
/// solver, kept as the reference the default strategy is measured against.
const PROFILE_TRACKS: [(Profile, Strategy, bool); 5] = [
    (Profile::Wordle, Strategy::DEFAULT, true),
    (Profile::WordleAll, Strategy::DEFAULT, false),
    (Profile::Wordle, pool(GuessPool::Hard), false),
    (Profile::Wordle, pool(GuessPool::Ultrahard), false),
    (Profile::Wordle, Strategy::ultrahard(), false),
];

/// The default Strategy with Guess pool `pool`.
const fn pool(pool: GuessPool) -> Strategy {
    Strategy {
        pool,
        ..Strategy::DEFAULT
    }
}

/// The Corpus lengths a language has Tracks for: 3 up to its maximum Supported length.
const fn corpus_track_count() -> usize {
    let mut count = 0;
    let mut l = 0;
    while l < Language::VARIANTS.len() {
        count += Language::VARIANTS[l].max_supported_length() - MIN_WORD_LENGTH + 1;
        l += 1;
    }
    count
}

/// Number of Corpus Tracks: every language at every Supported length.
const CORPUS_TRACK_COUNT: usize = corpus_track_count();
/// Number of Tracks in the table.
pub const TRACK_COUNT: usize = CORPUS_TRACK_COUNT + PROFILE_TRACKS.len();

/// How much a Track weighs in the benchmark (ADR 0007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Scored on the full dictionary with the tightest regression bound.
    Headline,
    /// Scored with a looser regression bound.
    Breadth,
    /// Only has to finish.
    Smoke,
}

impl Tier {
    /// The Tier a Corpus Track of this length has.
    #[must_use]
    pub const fn of_length(length: usize) -> Self {
        if length == HEADLINE_LENGTH {
            return Self::Headline;
        }
        let mut i = 0;
        while i < BREADTH_LENGTHS.len() {
            if BREADTH_LENGTHS[i] == length {
                return Self::Breadth;
            }
            i += 1;
        }
        Self::Smoke
    }

    /// The Tier's name in snapshots and the verdict table.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Headline => "headline",
            Self::Breadth => "breadth",
            Self::Smoke => "smoke",
        }
    }
}

/// The targets a Track plays; a Track never reads a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackTargets {
    /// Every answer.
    Full,
    /// `n` answers sampled with `seed`.
    Sample {
        /// How many answers.
        n: usize,
        /// Seed of the draw.
        seed: u64,
    },
}

impl From<TrackTargets> for Targets {
    fn from(targets: TrackTargets) -> Self {
        match targets {
            TrackTargets::Full => Self::Full,
            TrackTargets::Sample { n, seed } => Self::Sample { n, seed },
        }
    }
}

/// One scored pairing: a game, its Strategy and its targets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Track {
    /// The game played.
    pub game: Game,
    /// The Strategy it is played with.
    pub strategy: Strategy,
    /// Its Tier.
    pub tier: Tier,
    /// Whether the Track counts toward the verdict, or is only reported.
    pub judged: bool,
    /// The answers it plays.
    pub targets: TrackTargets,
}

impl Track {
    /// The run configuration that plays this Track.
    #[must_use]
    pub fn config(&self) -> RunConfig {
        RunConfig {
            game: self.game,
            strategy: self.strategy,
            targets: self.targets.into(),
        }
    }

    /// The Track with this id, as printed by `Display`.
    #[must_use]
    pub fn find(id: &str) -> Option<Self> {
        TRACKS.iter().find(|t| t.to_string() == id).copied()
    }
}

/// `{language}-{length}` for a Corpus game, the Profile's name for a Profile game, each followed
/// by `-{pool}` unless the pool is `all`. The original solver (ultrahard, win weight 0) is
/// `-original`, so it does not share an id with the ultrahard pool under the default strategy.
impl fmt::Display for Track {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.game {
            Game::Corpus {
                language, length, ..
            } => write!(f, "{language}-{length}")?,
            Game::Profile(profile) => write!(f, "{profile}")?,
        }
        if self.strategy == Strategy::ultrahard() {
            return write!(f, "-original");
        }
        match self.strategy.pool {
            GuessPool::All => Ok(()),
            pool => write!(f, "-{pool}"),
        }
    }
}

/// The targets a Corpus Track of `tier` and `length` plays.
const fn corpus_targets(tier: Tier, length: usize) -> TrackTargets {
    match tier {
        Tier::Headline => TrackTargets::Full,
        Tier::Breadth if length == FULL_BREADTH_LENGTH => TrackTargets::Full,
        Tier::Breadth => TrackTargets::Sample {
            n: BREADTH_SAMPLE,
            seed: SAMPLE_SEED,
        },
        Tier::Smoke => TrackTargets::Sample {
            n: SMOKE_SAMPLE,
            seed: SAMPLE_SEED,
        },
    }
}

/// Builds the Track table from the supported languages, lengths and Profiles.
const fn build() -> [Track; TRACK_COUNT] {
    let mut tracks = [Track {
        game: Game::Profile(Profile::Wordle),
        strategy: Strategy::DEFAULT,
        tier: Tier::Headline,
        judged: true,
        targets: TrackTargets::Full,
    }; TRACK_COUNT];
    let mut n = 0;
    let mut l = 0;
    while l < Language::VARIANTS.len() {
        let mut length = MIN_WORD_LENGTH;
        while length <= Language::VARIANTS[l].max_supported_length() {
            let tier = Tier::of_length(length);
            tracks[n] = Track {
                game: Game::Corpus {
                    language: Language::VARIANTS[l],
                    length,
                    dictionary_length: None,
                },
                strategy: Strategy::DEFAULT,
                tier,
                judged: !matches!(tier, Tier::Smoke),
                targets: corpus_targets(tier, length),
            };
            n += 1;
            length += 1;
        }
        l += 1;
    }
    let mut p = 0;
    while p < PROFILE_TRACKS.len() {
        let (profile, strategy, judged) = PROFILE_TRACKS[p];
        tracks[n] = Track {
            game: Game::Profile(profile),
            strategy,
            tier: Tier::Headline,
            judged,
            targets: TrackTargets::Full,
        };
        n += 1;
        p += 1;
    }
    tracks
}

/// Every Track.
pub const TRACKS: [Track; TRACK_COUNT] = build();

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_are_unique_and_found() {
        let ids: HashSet<String> = TRACKS.iter().map(Track::to_string).collect();
        assert_eq!(ids.len(), TRACK_COUNT);
        for track in &TRACKS {
            assert_eq!(Track::find(&track.to_string()), Some(*track));
        }
        assert_eq!(Track::find("english-5-ultrahard"), None);
    }

    #[test]
    fn ids_follow_the_documented_forms() {
        for id in [
            "english-5",
            "russian-10",
            "wordle",
            "wordle-all",
            "wordle-hard",
            "wordle-ultrahard",
            "wordle-original",
        ] {
            assert!(Track::find(id).is_some(), "{id}");
        }
    }

    #[test]
    fn the_count_follows_the_variants_and_the_length_range() {
        let lengths = |l: Language| l.max_supported_length() - MIN_WORD_LENGTH + 1;
        let corpus_total: usize = Language::VARIANTS.iter().map(|&l| lengths(l)).sum();
        assert_eq!(TRACK_COUNT, corpus_total + PROFILE_TRACKS.len());
        let corpus = TRACKS
            .iter()
            .filter(|t| matches!(t.game, Game::Corpus { .. }))
            .count();
        assert_eq!(corpus, corpus_total);
        for &language in Language::VARIANTS {
            for length in MIN_WORD_LENGTH..=language.max_supported_length() {
                assert!(
                    TRACKS.iter().any(|t| matches!(t.game,
                        Game::Corpus { language: l, length: n, .. } if l == language && n == length)),
                    "{language}-{length}"
                );
            }
        }
    }

    #[test]
    fn targets_follow_the_tier_rule() {
        for track in &TRACKS {
            let length = track.game.word_length();
            let expected = match track.tier {
                Tier::Headline => TrackTargets::Full,
                Tier::Breadth if length == 3 => TrackTargets::Full,
                Tier::Breadth => TrackTargets::Sample { n: 500, seed: 0 },
                Tier::Smoke => TrackTargets::Sample { n: 100, seed: 0 },
            };
            assert_eq!(track.targets, expected, "{track}");
        }
    }

    #[test]
    fn tiers_and_judging_follow_adr_0007() {
        for track in &TRACKS {
            match track.game {
                Game::Corpus { length, .. } => {
                    let tier = match length {
                        5 => Tier::Headline,
                        3 | 8 | 13 | 21 => Tier::Breadth,
                        _ => Tier::Smoke,
                    };
                    assert_eq!(track.tier, tier, "{track}");
                    assert_eq!(track.judged, tier != Tier::Smoke, "{track}");
                    assert_eq!(track.strategy, Strategy::DEFAULT, "{track}");
                }
                Game::Profile(_) => {
                    assert_eq!(track.tier, Tier::Headline, "{track}");
                    assert_eq!(track.judged, track.to_string() == "wordle", "{track}");
                }
            }
        }
    }

    #[test]
    fn the_original_solver_track_plays_ultrahard_with_no_win_weight() {
        let track = Track::find("wordle-original").unwrap();
        assert_eq!(track.strategy, Strategy::ultrahard());
        assert_eq!(track.game, Game::Profile(Profile::Wordle));
        assert_eq!(track.tier, Tier::Headline);
        assert!(!track.judged);
        assert_eq!(track.targets, TrackTargets::Full);
    }

    #[test]
    fn a_track_config_plays_its_own_game() {
        let track = Track::find("wordle-hard").unwrap();
        let config = track.config();
        assert_eq!(config.game, Game::Profile(Profile::Wordle));
        assert_eq!(config.strategy.pool, GuessPool::Hard);
        assert_eq!(config.targets, Targets::Full);
    }
}
