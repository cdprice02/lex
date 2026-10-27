use std::cell::RefCell;
use std::rc::Rc;

use lex_data::Word;
use lex_data::WordSet;

use crate::allowed::HardRule;
use crate::book::{BOOK_FLOOR, OpeningBook};
use crate::candidates::{Candidates, Suggestion};
use crate::correctness::WordCorrectness;
use crate::strategy::{GuessPool, Pool, Strategy};
use crate::tree::{DecisionTree, NodeId};

#[derive(Debug, Clone)]
pub struct Guess<const N: usize> {
    word: Word<N>,
    correctness: WordCorrectness<N>,
}

impl<const N: usize> Guess<N> {
    pub fn new(word: Word<N>, correctness: WordCorrectness<N>) -> Self {
        Self { word, correctness }
    }

    pub fn word(&self) -> Word<N> {
        self.word
    }

    pub fn correctness(&self) -> WordCorrectness<N> {
        self.correctness
    }
}

/// One game's solver state: the guesses played and a cursor into a shared decision tree that can be
/// reused across games with [`reset`](Self::reset), so repeated positions are scored once. The
/// [`Strategy`] is fixed at construction.
pub struct Guesser<const N: usize> {
    tree: Rc<RefCell<DecisionTree<N>>>,
    node: NodeId,
    history: Vec<Guess<N>>,
}

impl<const N: usize> Guesser<N> {
    /// `pool` is every word the solver may guess; `answers` are the only words the target can
    /// be, a subset of `pool` (all of it when nothing narrows the answers).
    ///
    /// # Panics
    ///
    /// If an answer is not in the pool, if the strategy's temperature is not positive and
    /// finite, or if its win weight is negative or not finite.
    pub fn new(pool: WordSet<N>, answers: Vec<Word<N>>, strategy: Strategy) -> Self {
        Self::build(pool, answers, strategy, None)
    }

    /// Like [`new`](Self::new), seeded with an [`OpeningBook`] built for the same pool, Answer
    /// list and strategy: a position on the book's path takes its booked guess without a search,
    /// and a guess off the path leaves the book, with everything below computed online. Play is
    /// identical to an unseeded guesser's.
    ///
    /// # Panics
    ///
    /// As [`new`](Self::new), and if the book names a guess outside the pool.
    pub fn with_book(
        pool: WordSet<N>,
        answers: Vec<Word<N>>,
        strategy: Strategy,
        book: OpeningBook<N>,
    ) -> Self {
        Self::build(pool, answers, strategy, Some(book))
    }

    fn build(
        pool: WordSet<N>,
        answers: Vec<Word<N>>,
        strategy: Strategy,
        book: Option<OpeningBook<N>>,
    ) -> Self {
        strategy.assert_valid();
        let pool = Pool::new(&pool);
        assert!(
            book.as_ref()
                .and_then(OpeningBook::max_guess)
                .is_none_or(|g| (g as usize) < pool.len()),
            "the opening book names a guess outside the pool"
        );
        let candidates = Candidates::new(pool, &answers);
        Self {
            tree: Rc::new(RefCell::new(DecisionTree::new(candidates, strategy, book))),
            node: DecisionTree::<N>::ROOT,
            history: Vec::new(),
        }
    }

    /// Builds the [`OpeningBook`] of this guesser's starting position: every position on the
    /// policy's paths whose work exceeds [`BOOK_FLOOR`].
    pub fn build_book(&self) -> OpeningBook<N> {
        self.build_book_with_floor(BOOK_FLOOR)
    }

    /// As [`build_book`](Self::build_book) with a chosen floor, in pattern evaluations (guesses
    /// the strategy allows times Candidate set size).
    pub fn build_book_with_floor(&self, floor: u64) -> OpeningBook<N> {
        self.tree.borrow_mut().build_book(floor)
    }

    pub fn with_history(
        pool: WordSet<N>,
        answers: Vec<Word<N>>,
        strategy: Strategy,
        history: Vec<Guess<N>>,
    ) -> Self {
        let mut guesser = Self::new(pool, answers, strategy);
        for guess in history {
            guesser.push_guess(guess);
        }
        guesser
    }

    /// Forgets the guesses played and returns to the starting position. The decision tree, and
    /// every suggestion already computed in it, is kept.
    pub fn reset(&mut self) {
        self.node = DecisionTree::<N>::ROOT;
        self.history.clear();
    }

    pub fn history(&self) -> &[Guess<N>] {
        &self.history
    }

    pub fn push_guess(&mut self, guess: Guess<N>) {
        self.node = self.tree.borrow_mut().child(self.node, &guess);
        self.history.push(guess);
    }

    /// Number of candidate words still consistent with the pushed guesses.
    pub fn num_candidates(&self) -> usize {
        self.tree.borrow().candidates(self.node).len()
    }

    /// The `k` best guesses by the strategy's score, descending. Equal scores prefer words still
    /// in the candidate set, then frequency order, so the first element always agrees with
    /// [`suggest`](Self::suggest). Not memoized.
    pub fn suggest_top_k(&self, k: usize) -> Vec<Suggestion<N>> {
        let tree = self.tree.borrow();
        tree.candidates(self.node)
            .suggest_top_k(tree.strategy(), tree.allowed(self.node), k)
    }

    /// Why `word` may not be played next under the strategy's pool rule and the guesses pushed,
    /// as a one-line reason, or `None` when it is allowed (always, outside hard and ultrahard
    /// mode). Independent of the tree, so it also vets a word the solver never scored.
    pub fn rule_violation(&self, word: Word<N>) -> Option<String> {
        let letters = |w: &Word<N>| -> [char; N] { std::array::from_fn(|i| w[i]) };
        let candidate = letters(&word);
        match self.tree.borrow().strategy().pool {
            GuessPool::All => None,
            GuessPool::Hard => self.history.iter().find_map(|guess| {
                HardRule::new(&letters(&guess.word()), guess.correctness().ordinal())
                    .violation(&candidate)
                    .map(|v| format!("hard mode: {v}"))
            }),
            GuessPool::Ultrahard => self
                .history
                .iter()
                .find(|g| WordCorrectness::correct(word, g.word()) != g.correctness())
                .map(|g| {
                    format!(
                        "ultrahard mode: must be consistent with the feedback for {}",
                        g.word()
                    )
                }),
        }
    }

    pub fn suggest(&self) -> Option<Word<N>> {
        self.tree.borrow_mut().suggest(self.node)
    }
}

/// A guesser whose answers are every pool word.
#[cfg(test)]
fn corpus_guesser<const N: usize>(pool: WordSet<N>, strategy: Strategy) -> Guesser<N> {
    let answers = pool.words().collect();
    Guesser::new(pool, answers, strategy)
}

#[cfg(test)]
mod benches {
    extern crate test;
    use std::collections::HashMap;
    use std::hint::black_box;
    use test::Bencher;

    use lex_data::{Word, WordSet};

    use super::*;

    fn make_wordset(n: usize) -> Vec<(Word<5>, u64)> {
        (0..n)
            .map(|i| {
                let s: String = [
                    char::from(b'a' + (i % 26) as u8),
                    char::from(b'a' + ((i / 26) % 26) as u8),
                    char::from(b'a' + ((i / 676) % 26) as u8),
                    char::from(b'a' + ((i / 17576) % 26) as u8),
                    char::from(b'a' + ((i / 456976) % 26) as u8),
                ]
                .iter()
                .collect();
                (Word::<5>::try_from(s.as_str()).unwrap(), (n - i) as u64)
            })
            .collect()
    }

    fn build_guesser(pairs: &[(Word<5>, u64)]) -> Guesser<5> {
        let frequencies: HashMap<Word<5>, u64> = pairs.iter().cloned().collect();
        let ws = WordSet::from_frequency_map(frequencies);
        corpus_guesser(ws, Strategy::ultrahard())
    }

    #[bench]
    fn suggest_50_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(50);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            g.push_guess(initial_guess.clone());
            black_box(g.suggest())
        });
    }

    #[bench]
    fn suggest_200_word_dict(b: &mut Bencher) {
        let pairs = make_wordset(200);
        let first_guess = pairs[0].0;
        let target = pairs[1].0;
        let initial_guess = Guess::new(first_guess, WordCorrectness::correct(target, first_guess));
        b.iter(|| {
            let mut g = build_guesser(&pairs);
            g.push_guess(initial_guess.clone());
            black_box(g.suggest())
        });
    }
}

#[cfg(test)]
mod tests {
    use lex_data::{Word, WordSet};

    use std::sync::Arc;

    use super::*;
    use crate::allowed::Allowed;

    fn make_guesser(pairs: &[(&str, u64)]) -> Guesser<5> {
        let frequencies = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        let ws = WordSet::from_frequency_map(frequencies);
        corpus_guesser(ws, Strategy::ultrahard())
    }

    fn word(s: &str) -> Word<5> {
        Word::<5>::try_from(s).unwrap()
    }

    fn make_guess(guess_str: &str, target_str: &str) -> Guess<5> {
        let guess_word = word(guess_str);
        let target_word = word(target_str);
        Guess::new(
            guess_word,
            WordCorrectness::correct(target_word, guess_word),
        )
    }

    #[test]
    fn suggest_no_prior_guesses_returns_word() {
        let all_words = ["crane", "stare", "light", "mount", "swipe"];
        let g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        let next = g.suggest().unwrap();
        assert!(
            all_words.iter().any(|&w| word(w) == next),
            "suggest returned {next}, which is not in the dictionary"
        );
    }

    #[test]
    fn suggest_returns_dictionary_member() {
        let all_words = ["crane", "stare", "light", "mount", "swipe"];
        let mut g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        g.push_guess(make_guess("crane", "stare"));
        let next = g.suggest().unwrap();
        assert!(
            all_words.iter().any(|&w| word(w) == next),
            "suggest returned {next}, which is not in the dictionary"
        );
    }

    //? this test might fail if we allow "impossible" words in the future for valuable information gain
    #[test]
    fn suggest_filters_impossible_words() {
        // words inconsistent with the last guess's correctness pattern are removed from the dictionary
        let mut g = make_guesser(&[
            ("crane", 100),
            ("crave", 100),
            ("craze", 100),
            ("graze", 100),
        ]);
        g.push_guess(make_guess("crane", "crave"));
        let next = g.suggest().unwrap();
        let valid = [word("crave"), word("craze")];
        assert!(
            valid.contains(&next),
            "expected crave or craze after filtering, got {next}"
        );
    }

    #[test]
    fn suggest_single_word() {
        let mut g = make_guesser(&[("crane", 100), ("crave", 200), ("graze", 100)]);
        g.push_guess(make_guess("crane", "crave"));
        let next = g.suggest().unwrap();
        assert_eq!(next, word("crave"));
    }

    #[test]
    fn suggest_exhausted_dictionary_returns_none() {
        let mut g = make_guesser(&[("crane", 100), ("stare", 100)]);
        // push a guess where the target matches neither word's pattern
        let impossible = Guess::new(word("crane"), WordCorrectness::absent());
        g.push_guess(impossible);
        assert_eq!(g.suggest(), None);
    }

    #[test]
    fn cumulative_filter_across_two_guesses() {
        // Verifies that push_guess applies each constraint immediately, so two pushes without
        // an intervening suggest() correctly narrow the candidate set by both constraints.
        let mut g = make_guesser(&[
            ("crane", 100),
            ("crave", 100),
            ("craze", 100),
            ("graze", 100),
            ("trace", 100),
        ]);
        g.push_guess(make_guess("crane", "crave"));
        g.push_guess(make_guess("craze", "crave"));
        assert_eq!(g.suggest().unwrap(), word("crave"));
    }

    #[test]
    fn top_k_first_matches_suggest() {
        let mut g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        g.push_guess(make_guess("crane", "stare"));
        let top = g.suggest_top_k(3);
        assert_eq!(top[0].word, g.suggest().unwrap());
    }

    #[test]
    fn top_k_sorted_descending_and_clamped() {
        let g = make_guesser(&[
            ("crane", 300),
            ("stare", 200),
            ("light", 100),
            ("mount", 75),
            ("swipe", 25),
        ]);
        let top = g.suggest_top_k(10);
        assert_eq!(top.len(), 5, "k larger than candidate count clamps");
        assert!(
            top.windows(2)
                .all(|w| w[0].entropy_bits >= w[1].entropy_bits),
            "scores must be sorted descending"
        );
    }

    #[test]
    fn top_k_empty_set_returns_empty() {
        let mut g = make_guesser(&[("crane", 100), ("stare", 100)]);
        g.push_guess(Guess::new(word("crane"), WordCorrectness::absent()));
        assert!(g.suggest_top_k(3).is_empty());
    }

    const DICT: &[(&str, u64)] = &[
        ("crane", 300),
        ("crave", 250),
        ("craze", 200),
        ("graze", 150),
        ("trace", 120),
        ("stare", 100),
        ("light", 80),
        ("mount", 60),
        ("swipe", 25),
        ("brace", 20),
    ];

    fn dict_set(pairs: &[(&str, u64)]) -> WordSet<5> {
        WordSet::from_pairs(
            pairs
                .iter()
                .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
                .collect(),
        )
    }

    /// The unmemoized solver: refilter and rescore from scratch after every guess.
    fn reference_sequence(
        word_set: &WordSet<5>,
        strategy: &Strategy,
        target: Word<5>,
    ) -> Vec<Word<5>> {
        let pool = Pool::new(word_set);
        let mut candidates = Candidates::whole_pool(Arc::clone(&pool));
        let mut allowed = Allowed::root(strategy.pool, true);
        let mut played = Vec::new();
        loop {
            let guess = candidates
                .suggest(strategy, &allowed)
                .expect("target stays a candidate");
            played.push(guess);
            let correctness = WordCorrectness::correct(target, guess);
            if correctness.is_correct() {
                return played;
            }
            allowed = allowed.after(
                strategy.pool,
                &pool,
                &pool.encode(&guess),
                correctness.ordinal(),
            );
            candidates = candidates.filtered(&Guess::new(guess, correctness));
        }
    }

    fn memoized_sequence(guesser: &mut Guesser<5>, target: Word<5>) -> Vec<Word<5>> {
        crate::play(target, guesser, Vec::new())
            .unwrap()
            .guesses()
            .iter()
            .map(Guess::word)
            .collect()
    }

    const STRATEGIES: [Strategy; 5] = [
        Strategy::ultrahard(),
        Strategy {
            pool: GuessPool::Hard,
            ..Strategy::ultrahard()
        },
        Strategy {
            pool: GuessPool::Hard,
            win_weight: 1.0,
            ..Strategy::ultrahard()
        },
        Strategy {
            pool: GuessPool::All,
            temperature: 5.0,
            win_weight: 0.0,
            endgame_threshold: 0,
            endgame_aversion: 0.0,
        },
        Strategy {
            pool: GuessPool::All,
            temperature: 2.0,
            win_weight: 1.5,
            endgame_threshold: 0,
            endgame_aversion: 0.0,
        },
    ];

    #[test]
    fn shared_tree_matches_unmemoized_solver_for_every_target() {
        let ws = dict_set(DICT);
        for strategy in STRATEGIES {
            let mut guesser = corpus_guesser(ws.clone(), strategy);
            // two passes: the second replays every game entirely from cached nodes
            for _ in 0..2 {
                for &(target, _) in DICT {
                    let target = word(target);
                    assert_eq!(
                        memoized_sequence(&mut guesser, target),
                        reference_sequence(&ws, &strategy, target),
                        "target {target}, {strategy:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn non_suggested_guess_leaves_other_paths_untouched() {
        let ws = dict_set(DICT);
        let mut shared = corpus_guesser(ws.clone(), Strategy::ultrahard());
        // populate the tree along the solver's own path first
        memoized_sequence(&mut shared, word("brace"));

        shared.reset();
        shared.push_guess(make_guess("light", "crave"));
        let mut fresh = corpus_guesser(ws, Strategy::ultrahard());
        fresh.push_guess(make_guess("light", "crave"));
        assert_eq!(shared.suggest(), fresh.suggest());
        assert_eq!(shared.num_candidates(), fresh.num_candidates());
    }

    #[test]
    fn same_feedback_from_different_guesses_stays_distinct() {
        // "crane" and "stare" both score all-absent against "light"; the edge key must carry
        // the guess word or the two paths would share a node
        let ws = dict_set(DICT);
        let mut shared = corpus_guesser(ws.clone(), Strategy::ultrahard());
        shared.push_guess(make_guess("crane", "light"));
        let after_crane = shared.num_candidates();
        shared.reset();
        shared.push_guess(make_guess("stare", "light"));

        let mut fresh = corpus_guesser(ws, Strategy::ultrahard());
        fresh.push_guess(make_guess("stare", "light"));
        assert_eq!(shared.num_candidates(), fresh.num_candidates());
        assert_ne!(after_crane, shared.num_candidates());
    }

    #[test]
    fn reset_clears_history() {
        let mut g = make_guesser(DICT);
        g.push_guess(make_guess("crane", "stare"));
        g.reset();
        assert!(g.history().is_empty());
        assert_eq!(g.num_candidates(), DICT.len());
    }

    const BATCH_FAMILY: &[(&str, u64)] = &[
        ("batch", 100),
        ("catch", 100),
        ("hatch", 100),
        ("latch", 100),
        ("match", 100),
        ("patch", 100),
        ("watch", 100),
        // no candidate, but its letters tell the family apart
        ("blimp", 1),
        ("chomp", 1),
        ("whelp", 1),
    ];

    /// Guesses to solve `target` after a first guess of "watch", which leaves the six `-atch`
    /// words as candidates and rules the three probe words out as answers.
    fn guesses_after_watch(strategy: Strategy, target: &str) -> usize {
        let mut guesser = corpus_guesser(dict_set(BATCH_FAMILY), strategy);
        let history = vec![make_guess("watch", target)];
        crate::play(word(target), &mut guesser, history)
            .unwrap()
            .num_guesses()
    }

    const CRANE_PAIRS: &[(&str, u64)] = &[
        ("crane", 500),
        ("crave", 400),
        ("crape", 300),
        ("crate", 250),
        ("slate", 200),
    ];

    fn top_words(g: &Guesser<5>) -> Vec<String> {
        let mut words: Vec<String> = g
            .suggest_top_k(20)
            .iter()
            .map(|s| s.word.to_string())
            .collect();
        words.sort();
        words
    }

    fn with_strategy(pool: GuessPool, answers: &[&str], pairs: &[(&str, u64)]) -> Guesser<5> {
        let answers = answers.iter().map(|w| word(w)).collect();
        Guesser::new(
            dict_set(pairs),
            answers,
            Strategy {
                pool,
                ..Strategy::ultrahard()
            },
        )
    }

    #[test]
    fn hard_mode_allows_a_replayed_gray_that_ultrahard_forbids() {
        let all: Vec<&str> = CRANE_PAIRS.iter().map(|p| p.0).collect();
        let mut hard = with_strategy(GuessPool::Hard, &all, CRANE_PAIRS);
        let mut ultra = with_strategy(GuessPool::Ultrahard, &all, CRANE_PAIRS);
        for g in [&mut hard, &mut ultra] {
            g.push_guess(make_guess("crane", "crave"));
        }
        // greens c, r, a, e stay; the gray N may be replayed, so crane itself is playable
        assert_eq!(top_words(&hard), ["crane", "crape", "crate", "crave"]);
        assert_eq!(top_words(&ultra), ["crape", "crate", "crave"]);
    }

    #[test]
    fn hard_mode_allowed_words_follow_the_rule_for_every_pushed_guess() {
        let mut g = with_strategy(
            GuessPool::Hard,
            &["crane", "crave", "crape", "crate", "slate"],
            CRANE_PAIRS,
        );
        g.push_guess(make_guess("slate", "crate"));
        // slate scores 0 0 2 2 2 against crate: a, t, e stay in place
        assert_eq!(top_words(&g), ["crate", "slate"]);
        assert_eq!(
            g.rule_violation(word("crane")),
            Some("hard mode: must keep T in position 4".into())
        );
        assert_eq!(g.rule_violation(word("crate")), None);
    }

    #[test]
    fn ultrahard_on_a_profile_plays_pool_words_consistent_with_the_feedback() {
        let mut g = with_strategy(GuessPool::Ultrahard, &["crave", "slate"], CRANE_PAIRS);
        g.push_guess(make_guess("crane", "crave"));
        assert_eq!(g.num_candidates(), 1);
        // crape is no answer here but scores crane exactly as crave does
        assert_eq!(top_words(&g), ["crape", "crate", "crave"]);
        assert_eq!(
            g.rule_violation(word("crane")),
            Some("ultrahard mode: must be consistent with the feedback for crane".into())
        );
        assert_eq!(g.rule_violation(word("crape")), None);
    }

    #[test]
    fn unconstrained_play_has_no_rule_violation() {
        let mut g = with_strategy(GuessPool::All, &["crave"], CRANE_PAIRS);
        g.push_guess(make_guess("crane", "crave"));
        assert_eq!(g.rule_violation(word("slate")), None);
    }

    #[test]
    fn every_guess_a_solver_plays_obeys_its_mode_and_the_game_is_won() {
        for pool in [GuessPool::Hard, GuessPool::Ultrahard] {
            for endgame_threshold in [0, 12] {
                let strategy = Strategy {
                    pool,
                    endgame_threshold,
                    ..Strategy::default()
                };
                let mut guesser = corpus_guesser(dict_set(DICT), strategy);
                for &(target, _) in DICT {
                    let target = word(target);
                    guesser.reset();
                    loop {
                        let next = guesser.suggest().unwrap();
                        assert_eq!(guesser.rule_violation(next), None, "{strategy:?}");
                        let feedback = WordCorrectness::correct(target, next);
                        guesser.push_guess(Guess::new(next, feedback));
                        if next == target {
                            break;
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn ultrahard_never_suggests_a_ruled_out_word() {
        let mut g = corpus_guesser(dict_set(BATCH_FAMILY), Strategy::ultrahard());
        g.push_guess(make_guess("watch", "batch"));
        let candidates = ["batch", "catch", "hatch", "latch", "match", "patch"];
        assert!(g.suggest_top_k(20).iter().all(|s| s.in_candidates));
        assert!(candidates.contains(&g.suggest().unwrap().to_string().as_str()));
    }

    #[test]
    fn unconstrained_probes_with_a_ruled_out_word() {
        let mut g = corpus_guesser(dict_set(BATCH_FAMILY), Strategy::default());
        g.push_guess(make_guess("watch", "batch"));
        let probe = g.suggest().unwrap();
        let top = g.suggest_top_k(1)[0];
        assert_eq!(top.word, probe);
        assert!(
            !top.in_candidates,
            "{probe} should be a non-candidate probe"
        );
        assert_eq!(top.p_answer, 0.0);
    }

    #[test]
    fn unconstrained_beats_hard_mode_on_the_batch_family() {
        let total = |strategy| {
            ["batch", "catch", "hatch", "latch", "match", "patch"]
                .iter()
                .map(|t| guesses_after_watch(strategy, t))
                .sum::<usize>()
        };
        let (hard, unconstrained) = (total(Strategy::ultrahard()), total(Strategy::default()));
        assert!(
            unconstrained < hard,
            "unconstrained {unconstrained} should beat hard mode {hard}"
        );
    }

    #[test]
    fn equal_scores_prefer_a_candidate() {
        // crane and stare split {crane, stare} identically; "light" ties at zero entropy against
        // nothing useful. Whichever the pool offers, the winner must be a candidate.
        let mut g = corpus_guesser(
            dict_set(&[("crane", 100), ("stare", 100), ("light", 100)]),
            Strategy::default(),
        );
        g.push_guess(make_guess("light", "crane"));
        assert!(g.suggest_top_k(1)[0].in_candidates);
        assert!(["crane", "stare"].contains(&g.suggest().unwrap().to_string().as_str()));
    }

    #[test]
    fn win_weight_favors_the_likelier_answer() {
        // two candidates with the same entropy as guesses; the heavier one should win once the
        // win bonus is on
        let strategy = Strategy {
            win_weight: 1.0,
            ..Strategy::default()
        };
        let g = corpus_guesser(dict_set(&[("crane", 1000), ("stare", 10)]), strategy);
        assert_eq!(g.suggest().unwrap(), word("crane"));
    }

    #[test]
    fn top_k_first_agrees_with_suggest_for_every_strategy() {
        for strategy in STRATEGIES {
            let mut g = corpus_guesser(dict_set(DICT), strategy);
            g.push_guess(make_guess("crane", "stare"));
            assert_eq!(
                g.suggest_top_k(1)[0].word,
                g.suggest().unwrap(),
                "{strategy:?}"
            );
        }
    }

    /// A floor low enough that the ten-word fixtures book several levels.
    const LOW_FLOOR: u64 = 4;

    /// Every strategy shape the book must serve: the three pools, with and without an Endgame.
    fn book_strategies() -> Vec<Strategy> {
        let mut all = Vec::new();
        for pool in [GuessPool::All, GuessPool::Hard, GuessPool::Ultrahard] {
            for endgame_threshold in [0, 3] {
                all.push(Strategy {
                    pool,
                    endgame_threshold,
                    ..Strategy::default()
                });
            }
        }
        all
    }

    /// The Answer lists to play over: the whole pool (a Corpus run) and a Profile's subset.
    fn book_answer_lists() -> [Vec<Word<5>>; 2] {
        let everyone = DICT.iter().map(|&(w, _)| word(w)).collect();
        let profile = ["crane", "crave", "graze", "stare", "light", "brace"]
            .iter()
            .map(|w| word(w))
            .collect();
        [everyone, profile]
    }

    fn games(guesser: &mut Guesser<5>, targets: &[Word<5>]) -> Vec<Vec<Word<5>>> {
        targets
            .iter()
            .map(|&t| memoized_sequence(guesser, t))
            .collect()
    }

    fn fresh(strategy: Strategy, answers: &[Word<5>]) -> Guesser<5> {
        Guesser::new(dict_set(DICT), answers.to_vec(), strategy)
    }

    fn seeded(strategy: Strategy, answers: &[Word<5>]) -> Guesser<5> {
        let book = fresh(strategy, answers).build_book_with_floor(LOW_FLOOR);
        assert!(
            !book.is_empty(),
            "{strategy:?}: the book should be non-empty"
        );
        Guesser::with_book(dict_set(DICT), answers.to_vec(), strategy, book)
    }

    #[test]
    fn a_seeded_guesser_plays_the_same_games_for_every_mode_and_answer_list() {
        for strategy in book_strategies() {
            for answers in book_answer_lists() {
                assert_eq!(
                    games(&mut seeded(strategy, &answers), &answers),
                    games(&mut fresh(strategy, &answers), &answers),
                    "{strategy:?}"
                );
            }
        }
    }

    #[test]
    fn a_seeded_run_computes_no_suggestion_at_a_booked_node() {
        for strategy in book_strategies() {
            for answers in book_answer_lists() {
                let mut guesser = seeded(strategy, &answers);
                games(&mut guesser, &answers);
                assert_eq!(
                    guesser.tree.borrow().booked_computations(),
                    0,
                    "{strategy:?}"
                );
            }
        }
    }

    #[test]
    fn an_off_path_guess_leaves_the_book_and_is_computed_online() {
        let strategy = Strategy::default();
        let answers = &book_answer_lists()[0];
        let mut seeded = seeded(strategy, answers);
        let mut plain = fresh(strategy, answers);
        assert_eq!(seeded.suggest(), plain.suggest());
        for g in [&mut seeded, &mut plain] {
            g.push_guess(make_guess("mount", "crave"));
        }
        assert_eq!(seeded.suggest(), plain.suggest());
        assert_eq!(seeded.num_candidates(), plain.num_candidates());
    }

    #[test]
    fn a_higher_floor_books_fewer_positions() {
        let g = corpus_guesser(dict_set(DICT), Strategy::default());
        let (low, high) = (g.build_book_with_floor(5), g.build_book_with_floor(50));
        assert!(!high.is_empty() && high.len() < low.len());
        assert!(
            g.build_book_with_floor(100).is_empty(),
            "work 100 is not above 100"
        );
    }

    #[test]
    fn the_production_floor_books_nothing_on_a_small_pool() {
        let g = corpus_guesser(dict_set(DICT), Strategy::default());
        assert!(g.build_book().is_empty());
    }

    /// The Wordle Profile's root work (12,972 guesses x 2,315 answers, 3.0e7) is below the
    /// production floor, so that Profile's book is empty; English-5's root (27,524 words each
    /// way, 7.6e8) is above it, so its top levels are booked.
    #[test]
    fn the_production_floor_leaves_the_wordle_profile_unbooked_and_books_english_5() {
        const { assert!(12_972 * 2_315 < crate::BOOK_FLOOR) };
        const { assert!(27_524 * 27_524 > crate::BOOK_FLOOR) };
    }

    /// A Wordle-shaped root (3,000 guesses x 1,400 answers, 4.2e6) has no book at the
    /// production floor and exactly its root once the floor is lowered below its work.
    #[test]
    fn a_profile_shaped_root_is_booked_only_below_its_work() {
        let letters = |i: usize| -> String {
            [i, i / 26, i / 676, i / 17576, 7]
                .iter()
                .map(|d| char::from(b'a' + (d % 26) as u8))
                .collect()
        };
        let pairs: Vec<(String, u64)> = (0..3000).map(|i| (letters(i), 3000 - i as u64)).collect();
        let borrowed: Vec<(&str, u64)> = pairs.iter().map(|(w, f)| (w.as_str(), *f)).collect();
        let answers: Vec<Word<5>> = borrowed[..1400].iter().map(|&(w, _)| word(w)).collect();
        for pool in [GuessPool::All, GuessPool::Hard, GuessPool::Ultrahard] {
            let strategy = Strategy {
                pool,
                ..Strategy::default()
            };
            let g = Guesser::new(dict_set(&borrowed), answers.clone(), strategy);
            assert!(g.build_book().is_empty(), "{pool:?}");
            assert_eq!(g.build_book_with_floor(4_000_000).len(), 1, "{pool:?}");
        }
    }

    proptest::proptest! {
        #[test]
        fn shared_tree_matches_unmemoized_solver(
            words in proptest::collection::hash_set("[a-d]{5}", 1..40),
        ) {
            let pairs: Vec<(String, u64)> = words
                .into_iter()
                .enumerate()
                .map(|(i, w)| (w, 1 + i as u64 * 7))
                .collect();
            let borrowed: Vec<(&str, u64)> = pairs.iter().map(|(w, f)| (w.as_str(), *f)).collect();
            let ws = dict_set(&borrowed);
            for strategy in STRATEGIES {
                let mut guesser = corpus_guesser(ws.clone(), strategy);
                for (target, _) in &borrowed {
                    let target = word(target);
                    proptest::prop_assert_eq!(
                        memoized_sequence(&mut guesser, target),
                        reference_sequence(&ws, &strategy, target)
                    );
                }
            }
        }

        #[test]
        fn target_always_survives_filter(target in "[a-z]{5}", guess_str in "[a-z]{5}") {
            let target_word = Word::<5>::try_from(target.as_str()).unwrap();
            let guess_word = Word::<5>::try_from(guess_str.as_str()).unwrap();
            let feedback = WordCorrectness::correct(target_word, guess_word);

            let mut g = make_guesser(&[(target.as_str(), 100)]);
            g.push_guess(Guess::new(guess_word, feedback));

            // correct(target, guess) == feedback by construction -> target always passes its own filter
            proptest::prop_assert_eq!(g.suggest(), Some(target_word));
        }
    }

    fn pool_set() -> WordSet<5> {
        dict_set(&[
            ("crane", 500),
            ("stare", 400),
            ("light", 300),
            ("mount", 200),
            ("swipe", 100),
        ])
    }

    #[test]
    fn answer_list_sets_the_starting_candidates_but_not_the_guess_pool() {
        let answers = vec![word("light"), word("stare")];
        let g = Guesser::new(pool_set(), answers, Strategy::default());
        assert_eq!(g.num_candidates(), 2);
        let top = g.suggest_top_k(5);
        assert_eq!(top.len(), 5, "every pool word is a legal guess");
        let in_candidates: Vec<_> = top.iter().filter(|s| s.in_candidates).collect();
        assert_eq!(in_candidates.len(), 2);
        assert!(in_candidates.iter().all(|s| s.p_answer > 0.0));
    }

    #[test]
    fn answer_list_order_does_not_matter() {
        let forward = Guesser::new(
            pool_set(),
            vec![word("stare"), word("light")],
            Strategy::default(),
        );
        let reverse = Guesser::new(
            pool_set(),
            vec![word("light"), word("stare")],
            Strategy::default(),
        );
        assert_eq!(forward.suggest_top_k(5), reverse.suggest_top_k(5));
    }

    #[test]
    fn history_narrows_within_the_answer_list() {
        let g = Guesser::with_history(
            pool_set(),
            vec![word("crane"), word("light"), word("stare")],
            Strategy::default(),
            vec![make_guess("mount", "stare")],
        );
        assert_eq!(g.num_candidates(), 1);
        assert_eq!(g.suggest(), Some(word("stare")));
    }

    #[test]
    #[should_panic(expected = "not in the guess pool")]
    fn answer_missing_from_the_pool_panics() {
        Guesser::new(pool_set(), vec![word("zebra")], Strategy::default());
    }
}
