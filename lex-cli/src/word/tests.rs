use super::lookup::{CorpusIndex, Lists, ProfileLists, corpus_fact};
use super::render::{input_cell, render_steps};
use super::*;
use lex_core::{PriorShape, Strategy, answer_prior};
use lex_data::{DataDir, Language, ProfileWords, Word, WordSet};

/// A temp data dir with `files` written under it.
fn dir_with(files: &[(&str, &str)]) -> (tempfile::TempDir, DataDir) {
    let tmp = tempfile::tempdir().unwrap();
    for (relative, body) in files {
        let path = tmp.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let dir = DataDir::new(tmp.path());
    (tmp, dir)
}

fn inputs(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn the_prior_is_the_default_strategys_power_shape() {
    assert_eq!(Strategy::DEFAULT.prior_shape, PriorShape::Power);
}

#[test]
fn input_is_normalized_like_the_build() {
    let facts = Facts::new("CRANE");
    assert_eq!(facts.word, "crane");
    assert!(facts.shape && facts.in_range);
    assert_eq!(input_cell(&facts), "CRANE");
    assert_eq!(input_cell(&Facts::new("crane")), "");
    assert!(!Facts::new("two words").shape);
    assert!(!Facts::new("ab").in_range);
    assert!(!Facts::new("").shape);
}

#[test]
fn a_bad_word_is_decided_without_touching_the_cache() {
    let (tmp, dir) = dir_with(&[]);
    let facts = look_up(&dir, Language::English, &inputs(&["a1b2c", "ab"])).unwrap();
    assert_eq!(
        verdict(&facts[0], Language::English),
        "not a word: not a single alphabetic token"
    );
    assert_eq!(
        verdict(&facts[1], Language::English),
        "length 2 is outside the Supported range 3..=21"
    );
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn nothing_cached_reads_not_cached_and_unknown() {
    let (tmp, dir) = dir_with(&[]);
    let facts = look_up(&dir, Language::English, &inputs(&["crane"])).unwrap();
    assert_eq!(facts[0].dictionary, Dictionary::NotCached);
    assert_eq!(facts[0].corpus, Corpus::NotCached);
    let steps = render_steps(&facts[0], Language::English, false);
    assert!(steps.contains(
        "no english Dictionary is cached; fetch it with `lex dict fetch --lang english`"
    ));
    assert!(steps.contains(
        "no english Corpus of length 5 is cached; build it with `lex corpus build --lang english`"
    ));
    assert!(steps.contains("verdict\tunknown: the Dictionary and the Corpus are not cached"));
    assert_eq!(
        std::fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "nothing was created"
    );
}

#[test]
fn a_dictionary_word_and_a_stranger_are_told_apart() {
    let (_tmp, dir) = dir_with(&[("dicts/eng.txt", "crane\nslate\n")]);
    let facts = look_up(&dir, Language::English, &inputs(&["crane", "zzzzz"])).unwrap();
    assert_eq!(facts[0].dictionary, Dictionary::Member);
    assert_eq!(facts[1].dictionary, Dictionary::Missing);
    assert_eq!(
        verdict(&facts[0], Language::English),
        "unknown: the Corpus is not cached"
    );
    assert_eq!(
        verdict(&facts[1], Language::English),
        "not in the english Dictionary"
    );
}

/// A Supported Corpus of `size` five-letter words, `crane` the most frequent.
fn corpus(size: usize) -> WordSet<5> {
    let mut pairs = vec![(Word::<5>::try_from("crane").unwrap(), 1_000_000)];
    for i in 0..size - 1 {
        let letters = |n: usize| char::from(b'a' + u8::try_from(n % 26).unwrap());
        let word: String = [i % 26, (i / 26) % 26, (i / 676) % 26, 1, 2]
            .into_iter()
            .map(letters)
            .collect();
        pairs.push((
            Word::<5>::try_from(word.as_str()).unwrap(),
            1000 - u64::try_from(i).unwrap().min(900),
        ));
    }
    WordSet::from_pairs(pairs)
}

#[test]
fn rank_frequency_and_prior_come_from_the_solvers_prior() {
    let set = corpus(300);
    let index = CorpusIndex::new(&set);
    let crane = Word::<5>::try_from("crane").unwrap();
    let seen = corpus_fact(Some(&set), Some(&index), &crane);
    let expected = answer_prior(&set, Strategy::DEFAULT.temperature)
        .into_iter()
        .find(|(w, _)| *w == crane)
        .unwrap()
        .1;
    let Corpus::Seen {
        rank,
        size,
        frequency,
        prior,
    } = seen
    else {
        panic!("crane is in the Corpus");
    };
    assert_eq!((rank, size, frequency), (1, 300, 1_000_000));
    assert!((prior - expected).abs() < 1e-15);
    let stranger = Word::<5>::try_from("zzzzz").unwrap();
    assert_eq!(
        corpus_fact(Some(&set), Some(&index), &stranger),
        Corpus::Unseen
    );
}

#[test]
fn a_short_corpus_is_unsupported() {
    let set = corpus(50);
    let index = CorpusIndex::new(&set);
    let crane = Word::<5>::try_from("crane").unwrap();
    assert_eq!(
        corpus_fact(Some(&set), Some(&index), &crane),
        Corpus::Unsupported { count: 50 }
    );
}

#[test]
fn a_profile_prior_is_over_its_answer_list_with_the_floor_for_unseen_answers() {
    let set = corpus(300);
    let index = CorpusIndex::new(&set);
    let words = ProfileWords {
        answers: vec!["crane".to_owned(), "zzzzz".to_owned()],
        guess_only: vec!["aahed".to_owned()],
    };
    let lists = ProfileLists::<5> {
        profile: Profile::Wordle,
        lists: Some(Lists::new(&words, Some(&index))),
    };
    let word = |s: &str| Word::<5>::try_from(s).unwrap();
    let ProfileStatus::Answer { prior, answers } = lists.status(&word("crane")) else {
        panic!("crane is an answer");
    };
    assert_eq!(answers, 2);
    let floored = answer_prior(
        &WordSet::from_pairs(vec![
            (word("crane"), 1_000_000),
            (word("zzzzz"), index.floor),
        ]),
        Strategy::DEFAULT.temperature,
    );
    assert!((prior.unwrap() - floored[0].1).abs() < 1e-15);
    assert_eq!(lists.status(&word("aahed")), ProfileStatus::GuessOnly);
    assert_eq!(lists.status(&word("slate")), ProfileStatus::Outside);
}

#[test]
fn rows_are_tab_separated_with_one_header_line() {
    let facts = vec![Facts::new("CRANE"), Facts::new("ab")];
    let out = render(&facts, Language::English, false);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0],
        "word\tinput\tlength\tdictionary\trank\tfrequency\tprior (default Strategy)\tprofiles\tverdict"
    );
    assert_eq!(lines.len(), 3);
    assert!(lines[1].starts_with("crane\tCRANE\t5\t-\t-\t-\t-\t-\t"));
    assert!(lines[2].contains("outside the Supported range"));
}

#[test]
fn the_step_report_shows_the_typed_form_only_when_it_differs() {
    let typed = render_steps(&Facts::new("CRANE"), Language::English, false);
    assert!(typed.contains("\ninput\tCRANE\tas typed\n"), "{typed}");
    assert!(typed.contains("\nword\tcrane\t\n"), "{typed}");
    let plain = render_steps(&Facts::new("crane"), Language::English, false);
    assert!(!plain.contains("input"), "{plain}");
}

#[test]
fn a_single_word_is_a_row_when_piped_and_steps_on_a_terminal() {
    let facts = vec![Facts::new("crane")];
    assert!(render(&facts, Language::English, false).starts_with("word\tinput\tlength"));
    let terminal = render(&facts, Language::English, true);
    assert!(terminal.contains('\u{256d}') && terminal.contains("step"));
}
