use std::collections::BTreeMap;
use std::fmt::Write as _;

use lex_core::{GuessPool, Strategy};
use serde::{Deserialize, Serialize};

/// Real Wordle allows six guesses; a game needing more counts as a failure. The solver itself
/// keeps playing until it wins, so the true guess count is always recorded.
pub const MAX_GUESSES: usize = 6;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub run: Run,
    pub corpus: Corpus,
    pub summary: Summary,
    pub games: Vec<Game>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Run {
    pub lex_version: String,
    pub timestamp: String,
    pub seed: u64,
    pub num_games: usize,
    pub targets_file: Option<String>,
    pub first_guess: String,
    /// Reports from before the strategy was configurable played hard mode at the defaults.
    #[serde(default = "StrategyReport::legacy")]
    pub strategy: StrategyReport,
}

/// The solver [`Strategy`] a run played. Two reports are a like-for-like solver comparison only
/// when this matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyReport {
    pub hard_mode: bool,
    pub temperature: f64,
    pub win_weight: f64,
}

impl StrategyReport {
    fn legacy() -> Self {
        Strategy::hard_mode().into()
    }
}

impl From<Strategy> for StrategyReport {
    fn from(s: Strategy) -> Self {
        Self {
            hard_mode: s.pool == GuessPool::Candidates,
            temperature: s.temperature,
            win_weight: s.win_weight,
        }
    }
}

impl std::fmt::Display for StrategyReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}, temperature {}, win weight {}",
            if self.hard_mode {
                "hard mode"
            } else {
                "unconstrained"
            },
            self.temperature,
            self.win_weight
        )
    }
}

/// Identity of the data a run was measured against. Two reports are only comparable when
/// `ngrams_sha256` matches.
#[derive(Debug, Serialize, Deserialize)]
pub struct Corpus {
    pub language: String,
    pub word_length: usize,
    pub dictionary_length: usize,
    pub ngrams_snapshot: String,
    pub ngrams_sha256: String,
    pub dict: Option<DictProvenance>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DictProvenance {
    pub source_url: Option<String>,
    pub fetched_at: Option<String>,
    pub source_sha256: Option<String>,
    pub word_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Game {
    pub word: String,
    pub num_guesses: usize,
    /// The target's probability under the answer prior over the full dictionary.
    pub prior: f64,
    pub guesses: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub max_guesses: usize,
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
    pub mean: f64,
    pub failure_rate: f64,
    pub histogram: BTreeMap<usize, f64>,
}

impl Stats {
    fn weighted(games: &[Game], weight: impl Fn(&Game) -> f64) -> Self {
        let total: f64 = games.iter().map(&weight).sum();
        let mut mean = 0.0;
        let mut failure_rate = 0.0;
        let mut histogram = BTreeMap::new();
        for game in games {
            let share = weight(game) / total;
            mean += share * game.num_guesses as f64;
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
        let n = self.games.len();
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
                let bar = "#".repeat((share * 40.0).round() as usize);
                let count = if exact {
                    format!(" ({})", (share * n as f64).round() as usize)
                } else {
                    String::new()
                };
                let _ = writeln!(out, "  {guesses:>2} | {:>6.2}%{count} {bar}", share * 100.0);
            }
        }
        out
    }
}

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}
