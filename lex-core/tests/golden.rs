//! Exact guess sequences over a small fixed dictionary. Any solver-behavior change (scoring,
//! tie-breaking, filtering) shows up here as a diff; update the table only when the change is
//! intended and its effect has been measured against the committed baselines.

use lex_core::{Guesser, Strategy, play};
use lex_data::{Word, WordSet};

const FIXTURE: &str = include_str!("fixtures/words5.txt");

fn fixture() -> WordSet<5> {
    let pairs = FIXTURE
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (word, freq) = l.split_once(' ').expect("`word frequency` line");
            (
                Word::<5>::try_from(word).expect("five-letter word"),
                freq.parse().expect("integer frequency"),
            )
        })
        .collect();
    WordSet::from_pairs(pairs)
}

fn sequence(strategy: Strategy, target: &str) -> Vec<String> {
    let mut guesser = Guesser::new(fixture(), None, strategy);
    let result = play(
        Word::<5>::try_from(target).unwrap(),
        &mut guesser,
        Vec::new(),
    )
    .unwrap();
    result
        .guesses()
        .iter()
        .map(|g| g.word().to_string())
        .collect()
}

#[test]
fn hard_mode_guess_sequences_are_stable() {
    let expected: &[(&str, &[&str])] = &[
        ("bound", &["their", "sound", "bound"]),
        ("found", &["their", "sound", "bound", "found"]),
        ("hound", &["their", "hound"]),
        (
            "mound",
            &[
                "their", "sound", "bound", "found", "pound", "wound", "mound",
            ],
        ),
        ("pound", &["their", "sound", "bound", "found", "pound"]),
        ("round", &["their", "round"]),
        ("sound", &["their", "sound"]),
        (
            "wound",
            &["their", "sound", "bound", "found", "pound", "wound"],
        ),
        (
            "batch",
            &["their", "watch", "match", "catch", "patch", "batch"],
        ),
        ("catch", &["their", "watch", "match", "catch"]),
        (
            "hatch",
            &[
                "their", "watch", "match", "catch", "patch", "batch", "hatch",
            ],
        ),
        (
            "latch",
            &[
                "their", "watch", "match", "catch", "patch", "batch", "hatch", "latch",
            ],
        ),
        ("match", &["their", "watch", "match"]),
        ("patch", &["their", "watch", "match", "catch", "patch"]),
        ("watch", &["their", "watch"]),
        ("light", &["their", "night", "light"]),
        ("might", &["their", "night", "light", "might"]),
        ("night", &["their", "night"]),
        ("right", &["their", "right"]),
        ("sight", &["their", "night", "light", "might", "sight"]),
        ("tight", &["their", "tight"]),
        (
            "fight",
            &["their", "night", "light", "might", "sight", "fight"],
        ),
        ("about", &["their", "about"]),
        ("other", &["their", "other"]),
        ("which", &["their", "which"]),
        ("their", &["their"]),
    ];
    for (target, guesses) in expected {
        assert_eq!(
            &sequence(Strategy::hard_mode(), target),
            guesses,
            "target {target}"
        );
    }
}

/// The default strategy probes with words that are not themselves possible answers (here "mound"
/// and "which"), which hard mode never does.
#[test]
fn default_strategy_guess_sequences_are_stable() {
    let expected = "
bound: their sound bound
found: their sound bound found
hound: their hound
mound: their sound bound found pound wound mound
pound: their sound bound found pound
round: their round
sound: their sound
wound: their sound bound found pound wound
batch: their which match catch patch batch
catch: their which match catch
hatch: their which hatch
latch: their which match catch patch batch latch
match: their which match
patch: their which match catch patch
watch: their which watch
light: their mound light
might: their mound might
night: their mound night
right: their right
sight: their mound light sight
tight: their tight
fight: their mound light sight fight
about: their about
other: their other
which: their which
their: their
";
    for line in expected.trim().lines() {
        let (target, guesses) = line.split_once(": ").unwrap();
        let guesses: Vec<&str> = guesses.split(' ').collect();
        assert_eq!(
            sequence(Strategy::default(), target),
            guesses,
            "target {target}"
        );
    }
}
