//! Why the solver chose a guess: the choice, the guesses it weighed and the feedback it expects.
//!
//! The solver records these at decision time, from the scores its own code paths computed, when a
//! [`Guesser`](crate::Guesser) is switched to explain; with the switch off none of this runs.

use std::sync::Arc;

use lex_data::Word;
use strum::Display;

use crate::allowed::Allowed;
use crate::candidates::Scored;
use crate::correctness::WordCorrectness;
use crate::policy::BasePolicy;
use crate::pool_ops::PoolOps;
use crate::rollout::Rollout;
use crate::strategy::Strategy;

/// How many guesses an entropy explanation lists.
pub const LISTED: usize = 10;

/// The method that chose a guess at a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum Technique {
    /// The best entropy score.
    Entropy,
    /// The cheapest play-out among the top entropy-scored guesses.
    Rollout,
    /// The exact search of a small Candidate set.
    Endgame,
}

impl Technique {
    /// The byte an Opening book stores for the technique.
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Entropy => 0,
            Self::Rollout => 1,
            Self::Endgame => 2,
        }
    }

    /// The technique a stored byte names, `None` for a byte no build writes.
    pub(crate) const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Entropy),
            1 => Some(Self::Rollout),
            2 => Some(Self::Endgame),
            _ => None,
        }
    }
}

/// A guess scored against the Candidate set of a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoredGuess<const N: usize> {
    /// The guess.
    pub word: Word<N>,
    /// Expected information gain under the Answer prior, in bits.
    pub expected_bits: f64,
    /// The expected Candidate set size after the guess's feedback, under the Answer prior.
    pub expected_remaining: f64,
    /// The expected guesses to finish counting this one, where the Endgame or a Rollout computed
    /// it; `None` where only the entropy score decided.
    pub guess_cost: Option<f64>,
    /// The entropy score the solver ranks by: bits plus the win-weighted chance of the answer.
    pub score: f64,
    /// Whether the word is still consistent with every guess played.
    pub in_candidates: bool,
    /// Whether the Strategy's Guess pool rule allows playing the word here.
    pub permitted: bool,
}

/// One feedback pattern the chosen guess can earn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternShare<const N: usize> {
    /// The feedback.
    pub pattern: WordCorrectness<N>,
    /// How many Candidates earn it.
    pub count: usize,
    /// The chance of it under the Answer prior.
    pub probability: f64,
}

/// The solver's choice at a position and what it weighed.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation<const N: usize> {
    /// The guess [`suggest`](crate::Guesser::suggest) plays.
    pub guess: Word<N>,
    /// The method that chose it.
    pub technique: Technique,
    /// Whether the position came from the Opening book, whose stored technique and top guesses
    /// this explains with.
    pub from_book: bool,
    /// The chosen guess first, then the others the technique weighed (at most 10 from the
    /// book).
    pub guesses: Vec<ScoredGuess<N>>,
    /// The chosen guess's feedback patterns, ascending by pattern.
    pub distribution: Vec<PatternShare<N>>,
}

/// A feedback group of the chosen guess, by pattern ordinal.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    /// The pattern's ordinal.
    pub ordinal: u64,
    /// Candidates in the group.
    pub count: usize,
    /// The group's share of the prior.
    pub probability: f64,
}

/// A listed guess by pool index.
#[derive(Debug, Clone, Copy)]
pub struct Listed {
    /// The guess's pool index.
    pub index: u32,
    /// Expected bits.
    pub bits: f64,
    /// Expected remaining.
    pub remaining: f64,
    /// Guess cost, where computed.
    pub cost: Option<f64>,
    /// The entropy score.
    pub score: f64,
    /// Whether it is a candidate.
    pub in_candidates: bool,
}

/// What a node recorded when it decided.
#[derive(Debug, Clone)]
pub struct Record {
    /// The deciding method.
    pub technique: Technique,
    /// The guesses, the chosen one first.
    pub listed: Vec<Listed>,
    /// The chosen guess's feedback groups.
    pub distribution: Vec<Group>,
}

/// A guess to list: its pool index, the score the solver already kept for it, its Guess cost.
type Pick = (u32, Option<Scored>, Option<f64>);

/// One partition's summary.
struct Stats {
    /// Expected bits.
    bits: f64,
    /// Expected Candidate set size after the feedback.
    remaining: f64,
    /// The feedback groups, ascending by ordinal.
    groups: Vec<Group>,
}

/// Records and scores guesses against the Answer prior over the pool.
#[derive(Debug)]
pub struct Explainer {
    /// The Guess pool the indices point into.
    pool: Arc<dyn PoolOps>,
    /// Answer prior weight of each pool word, unnormalized.
    weights: Vec<f64>,
}

/// A count as a float.
fn count_f64(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

impl Explainer {
    /// An explainer over `pool` weighting answers at `temperature`.
    pub(crate) fn new(pool: Arc<dyn PoolOps>, temperature: f64) -> Self {
        let weights = pool.weights(temperature);
        Self { pool, weights }
    }

    /// The Answer prior weight of the pool word at `index`.
    fn weight(&self, index: u32) -> f64 {
        self.weights[usize::try_from(index).unwrap_or(usize::MAX)]
    }

    /// The summed prior weight of `set`.
    fn mass(&self, set: &[u32]) -> f64 {
        set.iter().map(|&i| self.weight(i)).sum()
    }

    /// The partition of `set` by `guess`: expected bits, expected remaining and the groups.
    fn stats(&self, set: &[u32], guess: u32) -> Stats {
        let mut keyed = Vec::with_capacity(set.len());
        self.pool.partition(set, guess, &mut keyed);
        let mass = self.mass(set);
        let mut bits = 0.0;
        let mut remaining = 0.0;
        let mut groups = Vec::new();
        for group in keyed.chunk_by(|a, b| a.0 == b.0) {
            let share: f64 = group.iter().map(|&(_, i)| self.weight(i)).sum();
            let probability = share / mass;
            bits = (-probability).mul_add(probability.log2(), bits);
            remaining = probability.mul_add(count_f64(group.len()), remaining);
            groups.push(Group {
                ordinal: group[0].0,
                count: group.len(),
                probability,
            });
        }
        Stats {
            bits,
            remaining,
            groups,
        }
    }

    /// The feedback groups of `guess` against `set`.
    pub(crate) fn distribution(&self, set: &[u32], guess: u32) -> Vec<Group> {
        self.stats(set, guess).groups
    }

    /// `guess` scored against `set`. `kept` is the score the solver already computed, which is
    /// used as it is; without it the bits come from the partition.
    fn list(
        &self,
        set: &[u32],
        strategy: &Strategy,
        (guess, kept, cost): Pick,
    ) -> (Listed, Vec<Group>) {
        let stats = self.stats(set, guess);
        let (bits, score, in_candidates) = kept.map_or_else(
            || {
                let in_candidates = set.binary_search(&guess).is_ok();
                let p_answer = if in_candidates {
                    self.weight(guess) / self.mass(set)
                } else {
                    0.0
                };
                (
                    stats.bits,
                    strategy.win_weight.mul_add(p_answer, stats.bits),
                    in_candidates,
                )
            },
            |s| {
                (
                    s.entropy_bits,
                    s.score(strategy.win_weight),
                    s.in_candidates,
                )
            },
        );
        let listed = Listed {
            index: guess,
            bits,
            remaining: stats.remaining,
            cost,
            score,
            in_candidates,
        };
        (listed, stats.groups)
    }

    /// Chooses the guess for `set` as the solver does and records what it weighed: the Endgame
    /// within its threshold, a Rollout in its band, otherwise the entropy scan. The first listed
    /// guess is the choice.
    pub(crate) fn decide(
        &self,
        set: &[u32],
        allowed: &Allowed,
        base: &BasePolicy,
        rollout: Option<&Rollout>,
    ) -> Option<Record> {
        let strategy = *base.strategy();
        let size = set.len();
        let (technique, picks): (Technique, Vec<Pick>) = if size == 0 {
            return None;
        } else if size == 1 {
            (Technique::Entropy, vec![(set[0], None, None)])
        } else if base.uses_endgame(size) {
            let (pick, cost) = base.endgame_pick(set, allowed)?;
            let others = set.iter().filter(|&&c| c != pick).map(|&c| (c, None, None));
            (
                Technique::Endgame,
                std::iter::once((pick, None, Some(cost)))
                    .chain(others)
                    .collect(),
            )
        } else if let Some(rollout) = rollout.filter(|r| r.applies(size)) {
            let (_, mut played) = rollout.best_guess_scored(set, allowed, base)?;
            // stable, so an exact tie keeps the higher-scored guess first, as the choice does
            played.sort_by(|a, b| a.1.total_cmp(&b.1));
            (
                Technique::Rollout,
                played
                    .into_iter()
                    .map(|(s, cost)| (s.index, Some(s), Some(cost)))
                    .collect(),
            )
        } else {
            let top = self.pool.top_k(set, &strategy, allowed, LISTED);
            (
                Technique::Entropy,
                top.into_iter().map(|s| (s.index, Some(s), None)).collect(),
            )
        };
        let mut listed = Vec::with_capacity(picks.len());
        let mut distribution = Vec::new();
        for pick in picks {
            let (entry, groups) = self.list(set, &strategy, pick);
            if listed.is_empty() {
                distribution = groups;
            }
            listed.push(entry);
        }
        (!listed.is_empty()).then_some(Record {
            technique,
            listed,
            distribution,
        })
    }

    /// `guess` scored on demand: bits and expected remaining always, a Guess cost from the Endgame
    /// or the Rollout's warm memo where one applies to `set`.
    pub(crate) fn score(
        &self,
        set: &[u32],
        allowed: &Allowed,
        base: &BasePolicy,
        rollout: Option<&Rollout>,
        guess: u32,
    ) -> Listed {
        let size = set.len();
        if size == 0 {
            return Listed {
                index: guess,
                bits: 0.0,
                remaining: 0.0,
                cost: None,
                score: 0.0,
                in_candidates: false,
            };
        }
        let cost = if base.uses_endgame(size) {
            base.endgame_cost(set, allowed, guess)
        } else {
            rollout
                .filter(|r| r.applies(size))
                .map(|r| r.cost_of(set, allowed, guess, base))
        };
        self.list(set, base.strategy(), (guess, None, cost)).0
    }
}
