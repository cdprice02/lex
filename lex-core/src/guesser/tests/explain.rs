//! Tests for the explanation API: what a position records and how it relates to `suggest`.

use super::*;
use crate::Technique;

/// A fixture Strategy of each Technique: with 10 candidates at the root, a threshold of 3 leaves
/// the root to Entropy (rollout off) or Rollout, and the positions of 2 or 3 to the Endgame.
fn technique_strategy(rollout_k: usize) -> Strategy {
    Strategy {
        endgame_threshold: 3,
        rollout_k,
        ..Strategy::default()
    }
}

fn explaining(strategy: Strategy) -> Guesser<5> {
    let mut g = fresh(strategy, &book_answer_lists()[0]);
    g.set_explain(true);
    g
}

/// The Answer prior renormalized over `candidates`, from the public `answer_prior`.
fn prior_over(strategy: &Strategy, candidates: &[Word<5>]) -> Vec<(Word<5>, f64)> {
    let all = crate::answer_prior(&dict_set(DICT), strategy.temperature);
    let kept: Vec<_> = all
        .into_iter()
        .filter(|(w, _)| candidates.contains(w))
        .collect();
    let total: f64 = kept.iter().map(|&(_, p)| p).sum();
    kept.into_iter().map(|(w, p)| (w, p / total)).collect()
}

/// (expected bits, Expected remaining) of `guess` over `prior`, grouped by feedback directly.
fn expectations(prior: &[(Word<5>, f64)], guess: Word<5>) -> (f64, f64) {
    let mut groups: std::collections::BTreeMap<u64, (f64, f64)> = std::collections::BTreeMap::new();
    for &(answer, p) in prior {
        let g = groups
            .entry(WordCorrectness::correct(answer, guess).ordinal())
            .or_default();
        g.0 += p;
        g.1 += 1.0;
    }
    let bits = groups.values().map(|&(p, _)| -p * p.log2()).sum();
    let remaining = groups.values().map(|&(p, n)| p * n).sum();
    (bits, remaining)
}

fn candidates_after(history: &[Guess<5>]) -> Vec<Word<5>> {
    DICT.iter()
        .map(|&(w, _)| word(w))
        .filter(|&w| {
            history
                .iter()
                .all(|g| WordCorrectness::correct(w, g.word()) == g.correctness())
        })
        .collect()
}

fn assert_close(a: f64, b: f64, what: &str) {
    assert!((a - b).abs() < 1e-9, "{what}: {a} vs {b}");
}

#[test]
fn an_entropy_explanation_lists_the_top_guesses_without_a_guess_cost() {
    let strategy = technique_strategy(0);
    let g = explaining(strategy);
    let explanation = g.explain().unwrap();
    assert_eq!(explanation.technique, Technique::Entropy);
    assert!(!explanation.from_book);
    assert_eq!(explanation.guess, g.suggest().unwrap());
    assert_eq!(explanation.guesses[0].word, explanation.guess);
    assert_eq!(explanation.guesses.len(), 10);
    assert!(
        explanation
            .guesses
            .windows(2)
            .all(|w| w[0].score >= w[1].score)
    );
    let prior = prior_over(&strategy, &candidates_after(&[]));
    for listed in &explanation.guesses {
        assert_eq!(listed.guess_cost, None);
        assert!(listed.permitted);
        let (bits, remaining) = expectations(&prior, listed.word);
        assert_close(listed.expected_bits, bits, "bits");
        assert_close(listed.expected_remaining, remaining, "remaining");
    }
    let top: Vec<_> = g.suggest_top_k(10).iter().map(|s| s.word).collect();
    let listed: Vec<_> = explanation.guesses.iter().map(|s| s.word).collect();
    assert_eq!(listed, top);
}

#[test]
fn the_distribution_is_the_chosen_guess_feedback_in_ascending_pattern_order() {
    let g = explaining(technique_strategy(0));
    let explanation = g.explain().unwrap();
    let patterns: Vec<u64> = explanation
        .distribution
        .iter()
        .map(|s| s.pattern.ordinal())
        .collect();
    assert!(patterns.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(
        explanation
            .distribution
            .iter()
            .map(|s| s.count)
            .sum::<usize>(),
        DICT.len()
    );
    assert_close(
        explanation.distribution.iter().map(|s| s.probability).sum(),
        1.0,
        "probabilities",
    );
    for share in &explanation.distribution {
        let in_group = DICT
            .iter()
            .filter(|&&(w, _)| {
                WordCorrectness::correct(word(w), explanation.guess) == share.pattern
            })
            .count();
        assert_eq!(share.count, in_group);
    }
}

#[test]
fn a_rollout_explanation_lists_the_played_out_guesses_cheapest_first() {
    let g = explaining(technique_strategy(5));
    let explanation = g.explain().unwrap();
    assert_eq!(explanation.technique, Technique::Rollout);
    assert_eq!(explanation.guess, g.suggest().unwrap());
    assert_eq!(explanation.guesses[0].word, explanation.guess);
    assert_eq!(explanation.guesses.len(), 5);
    let costs: Vec<f64> = explanation
        .guesses
        .iter()
        .map(|s| s.guess_cost.unwrap())
        .collect();
    assert!(costs.windows(2).all(|w| w[0] <= w[1]), "{costs:?}");
    assert!(costs[0] >= 1.0);
    let mut top: Vec<_> = g.suggest_top_k(5).iter().map(|s| s.word).collect();
    let mut played: Vec<_> = explanation.guesses.iter().map(|s| s.word).collect();
    top.sort_by_key(ToString::to_string);
    played.sort_by_key(ToString::to_string);
    assert_eq!(
        played, top,
        "the rollout plays out the top by entropy score"
    );
}

#[test]
fn an_endgame_explanation_leads_with_the_pick_and_costs_only_the_pick() {
    let strategy = technique_strategy(5);
    let mut g = explaining(strategy);
    // a guess of the fixture that leaves two or three candidates
    let guess = ["crave", "craze", "trace", "brace", "stare", "graze"]
        .iter()
        .map(|w| make_guess(w, "crane"))
        .find(|guess| (2..=3).contains(&candidates_after(std::slice::from_ref(guess)).len()))
        .unwrap();
    let candidates = candidates_after(std::slice::from_ref(&guess));
    g.push_guess(guess);
    let explanation = g.explain().unwrap();
    assert_eq!(explanation.technique, Technique::Endgame);
    assert_eq!(explanation.guess, g.suggest().unwrap());
    assert_eq!(explanation.guesses[0].word, explanation.guess);
    assert!(explanation.guesses[0].guess_cost.is_some());
    assert!(
        explanation.guesses[1..]
            .iter()
            .all(|s| s.guess_cost.is_none())
    );
    let rest: Vec<Word<5>> = explanation.guesses[1..].iter().map(|s| s.word).collect();
    let in_pool_order: Vec<Word<5>> = DICT
        .iter()
        .map(|&(w, _)| word(w))
        .filter(|w| candidates.contains(w) && *w != explanation.guess)
        .collect();
    assert_eq!(rest, in_pool_order);
    let prior = prior_over(&strategy, &candidates);
    for listed in &explanation.guesses {
        let (bits, remaining) = expectations(&prior, listed.word);
        assert_close(listed.expected_bits, bits, "bits");
        assert_close(listed.expected_remaining, remaining, "remaining");
    }
}

#[test]
fn a_pairs_guess_cost_is_one_guess_to_win_and_two_otherwise() {
    let strategy = technique_strategy(5);
    let history = [make_guess("crane", "crave")];
    let survivors = candidates_after(&history);
    assert_eq!(
        survivors.len(),
        2,
        "the fixture leaves a pair: {survivors:?}"
    );
    let mut g = explaining(strategy);
    g.push_guess(history[0].clone());
    let prior = prior_over(&strategy, &survivors);
    let p = |w: Word<5>| prior.iter().find(|&&(x, _)| x == w).unwrap().1;
    let pick = g.explain().unwrap().guesses[0];
    let other = survivors.iter().copied().find(|&w| w != pick.word).unwrap();
    assert_close(
        pick.guess_cost.unwrap(),
        2.0f64.mul_add(p(other), p(pick.word)),
        "pair cost",
    );
}

#[test]
fn under_endgame_aversion_a_guess_cost_is_the_certainty_equivalent() {
    let aversion = 0.5_f64;
    let strategy = Strategy {
        endgame_aversion: aversion,
        ..technique_strategy(5)
    };
    let history = [make_guess("crane", "crave")];
    let survivors = candidates_after(&history);
    let mut g = explaining(strategy);
    g.push_guess(history[0].clone());
    let prior = prior_over(&strategy, &survivors);
    let p = |w: Word<5>| prior.iter().find(|&&(x, _)| x == w).unwrap().1;
    let pick = g.explain().unwrap().guesses[0];
    let other = survivors.iter().copied().find(|&w| w != pick.word).unwrap();
    let a = aversion.exp();
    let expected = (p(other) * a).mul_add(a, p(pick.word) * a).ln() / aversion;
    assert_close(pick.guess_cost.unwrap(), expected, "certainty equivalent");
}

#[test]
fn an_explanation_of_a_booked_position_is_the_whole_explanation() {
    let answers = &book_answer_lists()[0];
    let mut g = seeded(Strategy::default(), answers);
    g.set_explain(true);
    let explanation = g.explain().unwrap();
    assert!(explanation.from_book);
    assert!(!explanation.guesses.is_empty());
    assert_eq!(explanation.guesses[0].word, explanation.guess);
    assert_eq!(explanation.guess, g.suggest().unwrap());
    assert_eq!(
        explanation
            .distribution
            .iter()
            .map(|s| s.count)
            .sum::<usize>(),
        DICT.len()
    );
}

/// Every position of every game of `answers` under `strategy`, explained from a book read back
/// from its file and explained online with no book: the two must be equal, scores bit for bit,
/// apart from the `from_book` flag. Returns how many positions came from the book.
fn book_explanations_equal_online_ones(strategy: Strategy, answers: &[Word<5>]) -> usize {
    let mut booked = through_a_file(strategy, answers);
    let mut online = fresh(strategy, answers);
    booked.set_explain(true);
    online.set_explain(true);
    let mut from_book = 0;
    for &target in answers {
        booked.reset();
        online.reset();
        loop {
            let mut a = booked.explain().unwrap();
            let b = online.explain().unwrap();
            from_book += usize::from(a.from_book);
            a.from_book = false;
            assert_eq!(a, b, "{strategy:?} {target}");
            let guess = Guess::new(b.guess, WordCorrectness::correct(target, b.guess));
            if guess.correctness().is_correct() {
                break;
            }
            booked.push_guess(guess.clone());
            online.push_guess(guess);
        }
    }
    from_book
}

#[test]
fn a_booked_explanation_equals_the_online_one_at_every_booked_position() {
    for strategy in book_strategies() {
        for answers in book_answer_lists() {
            let from_book = book_explanations_equal_online_ones(strategy, &answers);
            assert!(
                from_book > 0,
                "{strategy:?}: no position came from the book"
            );
        }
    }
}

#[test]
fn a_book_loaded_without_scores_plays_alike_and_explains_nothing_at_booked_positions() {
    let strategy = Strategy::default();
    let answers = &book_answer_lists()[0];
    let book = fresh(strategy, answers)
        .build_book_with_floor(LOW_FLOOR)
        .without_scores();
    assert!(!book.has_scores());
    let mut plain = Guesser::with_book(&dict_set(DICT), answers, strategy, book);
    assert_eq!(
        games(&mut plain, answers),
        games(&mut fresh(strategy, answers), answers)
    );
    plain.reset();
    plain.set_explain(true);
    assert_eq!(plain.explain(), None);
}

#[test]
fn building_a_book_leaves_recording_as_it_was() {
    let g = fresh(Strategy::default(), &book_answer_lists()[0]);
    let _ = g.build_book_with_floor(LOW_FLOOR);
    assert_eq!(g.explain(), None);
    let mut recording = fresh(Strategy::default(), &book_answer_lists()[0]);
    recording.set_explain(true);
    let _ = recording.build_book_with_floor(LOW_FLOOR);
    assert!(recording.explain().is_some());
}

#[test]
fn nothing_is_explained_while_recording_is_off() {
    let g = fresh(technique_strategy(5), &book_answer_lists()[0]);
    assert_eq!(g.explain(), None);
    assert!(g.suggest().is_some());
    assert_eq!(g.explain(), None);
}

#[test]
fn an_explanation_agrees_with_suggest_at_every_position_of_every_game() {
    for strategy in book_strategies() {
        for answers in book_answer_lists() {
            for booked in [false, true] {
                let mut plain = fresh(strategy, &answers);
                let mut explained = if booked {
                    seeded(strategy, &answers)
                } else {
                    fresh(strategy, &answers)
                };
                explained.set_explain(true);
                for &target in &answers {
                    plain.reset();
                    explained.reset();
                    while let Some(guess) = plain.suggest() {
                        let explanation = explained.explain().unwrap();
                        assert_eq!(explanation.guess, guess, "{strategy:?} {target}");
                        assert_eq!(explained.suggest(), Some(guess));
                        let guess = Guess::new(guess, WordCorrectness::correct(target, guess));
                        if guess.correctness().is_correct() {
                            break;
                        }
                        plain.push_guess(guess.clone());
                        explained.push_guess(guess);
                    }
                }
            }
        }
    }
}

#[test]
fn score_matches_the_listed_guess_in_every_band() {
    for (rollout_k, played) in [(0, false), (5, false), (5, true)] {
        let mut g = explaining(technique_strategy(rollout_k));
        if played {
            g.push_guess(make_guess("crane", "craze"));
        }
        let explanation = g.explain().unwrap();
        for listed in &explanation.guesses {
            let scored = g.score(listed.word).unwrap();
            assert_close(scored.expected_bits, listed.expected_bits, "bits");
            assert_close(
                scored.expected_remaining,
                listed.expected_remaining,
                "remaining",
            );
            // the Endgame lists a cost for its pick only; `score` costs any word in the band
            if let Some(listed_cost) = listed.guess_cost {
                assert_close(scored.guess_cost.unwrap(), listed_cost, "cost");
            }
            assert_eq!(scored.guess_cost.is_some(), rollout_k > 0);
        }
    }
}

#[test]
fn score_prices_a_word_outside_the_list_and_costs_none_in_the_entropy_band() {
    let outside = word("brace");
    let entropy = explaining(technique_strategy(0));
    let scored = entropy.score(outside).unwrap();
    assert_eq!(scored.guess_cost, None);
    let prior = prior_over(&technique_strategy(0), &candidates_after(&[]));
    assert_close(
        scored.expected_bits,
        expectations(&prior, outside).0,
        "bits",
    );
    let rollout = explaining(technique_strategy(5));
    assert!(rollout.score(outside).unwrap().guess_cost.is_some());
    assert_eq!(rollout.score(word("zzzzz")), None);
}

#[test]
fn score_flags_a_word_the_guess_pool_forbids() {
    let strategy = Strategy {
        pool: GuessPool::Hard,
        ..technique_strategy(0)
    };
    let mut g = explaining(strategy);
    g.push_guess(make_guess("crane", "crave"));
    let forbidden = g.score(word("light")).unwrap();
    assert!(!forbidden.permitted);
    assert!(g.score(word("crave")).unwrap().permitted);
    assert!(g.rule_violation(word("light")).is_some());
}
