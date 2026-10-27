use std::cell::RefCell;
use std::rc::Rc;

use lex_data::Word;
use lex_data::WordSet;

use crate::candidates::{Candidates, Suggestion};
use crate::correctness::WordCorrectness;
use crate::strategy::{Pool, Strategy};
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
        strategy.assert_valid();
        let candidates = Candidates::new(Pool::new(&pool), &answers);
        Self {
            tree: Rc::new(RefCell::new(DecisionTree::new(candidates, strategy))),
            node: DecisionTree::<N>::ROOT,
            history: Vec::new(),
        }
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
        tree.candidates(self.node).suggest_top_k(tree.strategy(), k)
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
        corpus_guesser(ws, Strategy::hard_mode())
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

    use super::*;
    use crate::strategy::GuessPool;

    fn make_guesser(pairs: &[(&str, u64)]) -> Guesser<5> {
        let frequencies = pairs
            .iter()
            .map(|&(s, f)| (Word::<5>::try_from(s).unwrap(), f))
            .collect();
        let ws = WordSet::from_frequency_map(frequencies);
        corpus_guesser(ws, Strategy::hard_mode())
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
        WordSet::from_frequency_map(
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
        let mut candidates = Candidates::whole_pool(Pool::new(word_set));
        let mut played = Vec::new();
        loop {
            let guess = candidates
                .suggest(strategy)
                .expect("target stays a candidate");
            played.push(guess);
            let correctness = WordCorrectness::correct(target, guess);
            if correctness.is_correct() {
                return played;
            }
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

    const STRATEGIES: [Strategy; 3] = [
        Strategy::hard_mode(),
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
        let mut shared = corpus_guesser(ws.clone(), Strategy::hard_mode());
        // populate the tree along the solver's own path first
        memoized_sequence(&mut shared, word("brace"));

        shared.reset();
        shared.push_guess(make_guess("light", "crave"));
        let mut fresh = corpus_guesser(ws, Strategy::hard_mode());
        fresh.push_guess(make_guess("light", "crave"));
        assert_eq!(shared.suggest(), fresh.suggest());
        assert_eq!(shared.num_candidates(), fresh.num_candidates());
    }

    #[test]
    fn same_feedback_from_different_guesses_stays_distinct() {
        // "crane" and "stare" both score all-absent against "light"; the edge key must carry
        // the guess word or the two paths would share a node
        let ws = dict_set(DICT);
        let mut shared = corpus_guesser(ws.clone(), Strategy::hard_mode());
        shared.push_guess(make_guess("crane", "light"));
        let after_crane = shared.num_candidates();
        shared.reset();
        shared.push_guess(make_guess("stare", "light"));

        let mut fresh = corpus_guesser(ws, Strategy::hard_mode());
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

    #[test]
    fn hard_mode_never_suggests_a_ruled_out_word() {
        let mut g = corpus_guesser(dict_set(BATCH_FAMILY), Strategy::hard_mode());
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
        let (hard, unconstrained) = (total(Strategy::hard_mode()), total(Strategy::default()));
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
