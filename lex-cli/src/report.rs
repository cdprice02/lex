//! The JSON report of a simulation run and its human-readable rendering.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use lex_core::{GuessPool, Strategy};
use serde::{Deserialize, Serialize};

/// Real Wordle allows six guesses; a game needing more counts as a failure. The solver itself
/// keeps playing until it wins, so the true guess count is always recorded.
pub const MAX_GUESSES: u32 = 6;

/// Version of the JSON report layout.
pub const SCHEMA_VERSION: u32 = 1;

/// The JSON record of one simulation run.
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    /// [`SCHEMA_VERSION`] when written.
    pub schema_version: u32,
    /// How the run was configured.
    pub run: Run,
    /// The data the run was measured against.
    pub corpus: Corpus,
    /// The Profile played, if any. Its `summary.uniform` is the headline: the Report prior of a
    /// Wordle Profile is uniform over the Answer list.
    #[serde(default)]
    pub profile: Option<ProfileReport>,
    /// Aggregate results of the games.
    pub summary: Summary,
    /// Every game played, in play order.
    pub games: Vec<Game>,
}

/// The parameters of a run.
#[derive(Debug, Serialize, Deserialize)]
pub struct Run {
    /// Version of `lex` that wrote the report.
    pub lex_version: String,
    /// RFC 3339 time of the run.
    pub timestamp: String,
    /// Seed the targets were sampled with.
    pub seed: u64,
    /// Number of games played.
    pub num_games: usize,
    /// The targets file, when one was given.
    pub targets_file: Option<String>,
    /// The first guess played.
    pub first_guess: String,
    /// Reports from before the strategy was configurable played hard mode at the defaults.
    #[serde(default = "StrategyReport::legacy")]
    pub strategy: StrategyReport,
}

/// Which guesses a run was allowed to play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Any word in the Guess pool.
    Unconstrained,
    /// The official Wordle hard-mode rule.
    Hard,
    /// Only words consistent with every feedback so far.
    Ultrahard,
}

/// The solver [`Strategy`] a run played. Two reports are a like-for-like solver comparison only
/// when this matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawStrategyReport")]
pub struct StrategyReport {
    /// Which guesses the run could play.
    pub mode: Mode,
    /// Answer prior temperature.
    pub temperature: f64,
    /// Score bonus, in bits, per unit of probability that a guess is the answer.
    pub win_weight: f64,
    /// Reports from before the endgame search never used one.
    #[serde(default)]
    pub endgame_threshold: usize,
    /// Endgame risk aversion; zero minimizes plain expected guesses.
    #[serde(default)]
    pub endgame_aversion: f64,
    /// Reports from before rollout never used one.
    #[serde(default)]
    pub rollout_k: usize,
}

/// The wire shape: reports written before the mode field carry `hard_mode: bool`, and that
/// bool meant candidates only, which is ultrahard.
#[derive(Deserialize)]
struct RawStrategyReport {
    /// Absent in reports written before the mode field.
    mode: Option<Mode>,
    /// Present in reports written before the mode field; true meant ultrahard.
    hard_mode: Option<bool>,
    /// Answer prior temperature.
    temperature: f64,
    /// Score bonus, in bits, per unit of probability that a guess is the answer.
    win_weight: f64,
    /// Endgame threshold, zero when absent.
    #[serde(default)]
    endgame_threshold: usize,
    /// Endgame risk aversion, zero when absent.
    #[serde(default)]
    endgame_aversion: f64,
    /// Rollout width, zero when absent.
    #[serde(default)]
    rollout_k: usize,
}

impl From<RawStrategyReport> for StrategyReport {
    fn from(raw: RawStrategyReport) -> Self {
        let mode = raw.mode.unwrap_or_else(|| {
            if raw.hard_mode == Some(true) {
                Mode::Ultrahard
            } else {
                Mode::Unconstrained
            }
        });
        Self {
            mode,
            temperature: raw.temperature,
            win_weight: raw.win_weight,
            endgame_threshold: raw.endgame_threshold,
            endgame_aversion: raw.endgame_aversion,
            rollout_k: raw.rollout_k,
        }
    }
}

impl StrategyReport {
    /// The Strategy a report without one played: ultrahard at the defaults.
    fn legacy() -> Self {
        Strategy::ultrahard().into()
    }
}

impl From<Strategy> for StrategyReport {
    fn from(s: Strategy) -> Self {
        Self {
            mode: match s.pool {
                GuessPool::All => Mode::Unconstrained,
                GuessPool::Hard => Mode::Hard,
                GuessPool::Ultrahard => Mode::Ultrahard,
            },
            temperature: s.temperature,
            win_weight: s.win_weight,
            endgame_threshold: s.endgame_threshold,
            endgame_aversion: s.endgame_aversion,
            rollout_k: s.rollout_k,
        }
    }
}

impl std::fmt::Display for StrategyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}, temperature {}, win weight {}, endgame threshold {}, aversion {}, rollout K {}",
            match self.mode {
                Mode::Unconstrained => "unconstrained",
                Mode::Hard => "hard mode",
                Mode::Ultrahard => "ultrahard mode",
            },
            self.temperature,
            self.win_weight,
            self.endgame_threshold,
            self.endgame_aversion,
            self.rollout_k
        )
    }
}

/// Identity of the data a run was measured against. Two reports are only comparable when
/// `ngrams_sha256` matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Corpus {
    /// The Corpus language.
    pub language: String,
    /// Length of every word.
    pub word_length: usize,
    /// Number of words in the Guess pool.
    pub dictionary_length: usize,
    /// The Google Books Ngrams snapshot the Corpus was built from.
    pub ngrams_snapshot: String,
    /// The Corpus checksum.
    pub ngrams_sha256: String,
    /// Provenance of the Wiktionary dictionary, when it was recorded.
    pub dict: Option<DictProvenance>,
}

/// Identity of the Profile lists a run played. Two reports are only comparable when the name and
/// both checksums match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileReport {
    /// The Profile's stored name.
    pub name: String,
    /// SHA-256 of the Answer list.
    pub answers_sha256: String,
    /// SHA-256 of the guess-only list.
    pub guess_only_sha256: String,
    /// Number of words in the Answer list.
    pub answer_count: usize,
    /// Number of words in the Guess pool.
    pub guess_count: usize,
    /// Count assigned to Profile words the Corpus lacks (the Unseen-word floor).
    pub floor: u64,
    /// Profile words the Corpus lacked, which took the floor.
    pub floored: usize,
}

impl ProfileReport {
    /// Whether both reports played the same Profile lists.
    #[must_use]
    pub fn same_lists(&self, other: &Self) -> bool {
        (&self.name, &self.answers_sha256, &self.guess_only_sha256)
            == (&other.name, &other.answers_sha256, &other.guess_only_sha256)
    }
}

impl From<lex_data::ProfileStats> for ProfileReport {
    fn from(s: lex_data::ProfileStats) -> Self {
        Self {
            name: s.name,
            answers_sha256: s.answers_sha256,
            guess_only_sha256: s.guess_only_sha256,
            answer_count: s.answer_count,
            guess_count: s.guess_count,
            floor: s.floor,
            floored: s.floored,
        }
    }
}

/// Where the Wiktionary dictionary was downloaded from, and when.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictProvenance {
    /// The URL downloaded.
    pub source_url: Option<String>,
    /// RFC 3339 time of the download.
    pub fetched_at: Option<String>,
    /// SHA-256 of the compressed download.
    pub source_sha256: Option<String>,
    /// Number of words in the dictionary.
    pub word_count: usize,
}

/// One game played.
#[derive(Debug, Serialize, Deserialize)]
pub struct Game {
    /// The target.
    pub word: String,
    /// Guesses taken, the winning one included.
    pub num_guesses: u32,
    /// The target's probability under the answer prior over the full dictionary.
    pub prior: f64,
    /// The guesses played, in order.
    pub guesses: Vec<String>,
}

/// The results of a run under both weightings.
#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    /// Guesses a real game allows, [`MAX_GUESSES`].
    pub max_guesses: u32,
    /// Every target weighted equally.
    pub uniform: Stats,
    /// Targets weighted by their answer-prior probability, renormalized over the targets
    /// played. Exact for a full-dictionary run; a self-normalized estimate for a sample.
    pub prior: Stats,
}

/// `histogram` maps guess count to the weight share of games finishing in that many guesses
/// (shares sum to 1). `failure_rate` is the share finishing in more than [`MAX_GUESSES`].
#[derive(Debug, Serialize, Deserialize)]
pub struct Stats {
    /// Weighted mean of guesses taken.
    pub mean: f64,
    /// Share of games finishing in more than [`MAX_GUESSES`].
    pub failure_rate: f64,
    /// Share of games finishing in each guess count.
    pub histogram: BTreeMap<u32, f64>,
}

impl Stats {
    /// The [`Stats`] of `games` with each weighted by `weight`.
    fn weighted(games: &[Game], weight: impl Fn(&Game) -> f64) -> Self {
        let total: f64 = games.iter().map(&weight).sum();
        let mut mean = 0.0;
        let mut failure_rate = 0.0;
        let mut histogram = BTreeMap::new();
        for game in games {
            let share = weight(game) / total;
            mean = share.mul_add(f64::from(game.num_guesses), mean);
            if game.num_guesses > MAX_GUESSES {
                failure_rate += share;
            }
            *histogram.entry(game.num_guesses).or_insert(0.0) += share;
        }
        Self {
            mean,
            failure_rate,
            histogram,
        }
    }
}

impl Summary {
    /// Both weightings of `games`.
    #[must_use]
    pub fn from_games(games: &[Game]) -> Self {
        Self {
            max_guesses: MAX_GUESSES,
            uniform: Stats::weighted(games, |_| 1.0),
            prior: Stats::weighted(games, |g| g.prior),
        }
    }
}

impl Report {
    /// Human-readable summary: parameters, corpus identity, then both weightings.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let c = &self.corpus;
        let _ = writeln!(
            out,
            "{} games, {}-letter {} words (dictionary: {}), seed {}",
            self.run.num_games, c.word_length, c.language, c.dictionary_length, self.run.seed
        );
        if let Some(p) = &self.profile {
            let _ = writeln!(
                out,
                "profile: {} ({} answers, {} guesses, {} floored at {}); uniform is the headline",
                p.name, p.answer_count, p.guess_count, p.floored, p.floor
            );
        }
        let _ = writeln!(out, "strategy: {}", self.run.strategy);
        let _ = writeln!(out, "first guess: {}", self.run.first_guess);
        let _ = writeln!(out, "ngrams sha256: {}", short(&c.ngrams_sha256));
        if let Some(dict) = &c.dict {
            let _ = writeln!(
                out,
                "dict: {} words, fetched {}, sha256 {}",
                dict.word_count,
                dict.fetched_at.as_deref().unwrap_or("unknown"),
                dict.source_sha256.as_deref().map_or("unknown", short)
            );
        }
        let n: f64 = self.games.iter().map(|_| 1.0).sum();
        for (name, stats, exact) in [
            ("uniform", &self.summary.uniform, true),
            ("prior-weighted", &self.summary.prior, false),
        ] {
            let _ = writeln!(
                out,
                "\n{name}: mean {:.3} guesses, failure rate (>{}) {:.2}%",
                stats.mean,
                self.summary.max_guesses,
                stats.failure_rate * 100.0
            );
            for (guesses, share) in &stats.histogram {
                // `k` cells are filled when round(share * 40) >= k, that is share * 40 >= k - 0.5
                let bar: String = (1..=40_u8)
                    .take_while(|&k| f64::from(k) - 0.5 <= share * 40.0)
                    .map(|_| '#')
                    .collect();
                let count = if exact {
                    format!(" ({:.0})", share * n)
                } else {
                    String::new()
                };
                let _ = writeln!(out, "  {guesses:>2} | {:>6.2}%{count} {bar}", share * 100.0);
            }
        }
        out
    }
}

/// The first 12 characters of `hash`.
fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(json: &str) -> StrategyReport {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_old_hard_mode_bool_reads_as_ultrahard() {
        let old = r#"{"hard_mode": true, "temperature": 5.0, "win_weight": 0.0}"#;
        assert_eq!(read(old).mode, Mode::Ultrahard);
        let old = r#"{"hard_mode": false, "temperature": 5.0, "win_weight": 1.0}"#;
        assert_eq!(read(old).mode, Mode::Unconstrained);
    }

    #[test]
    fn the_mode_round_trips_and_wins_over_the_old_bool() {
        for pool in [GuessPool::All, GuessPool::Hard, GuessPool::Ultrahard] {
            let report = StrategyReport::from(Strategy {
                pool,
                ..Strategy::default()
            });
            let json = serde_json::to_string(&report).unwrap();
            assert!(!json.contains("hard_mode"));
            assert_eq!(read(&json), report);
        }
    }
}
