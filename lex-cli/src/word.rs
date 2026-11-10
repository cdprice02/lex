//! `lex word <WORD>... --lang L`: where a word stands on the path from input to Candidate.
//!
//! Reads disk only: nothing is fetched or built, and a cache that is not there reads "not cached"
//! with the later steps unknown.
//!
//! The path in order: shape (one alphabetic token after NFC and lowercasing), a length in the
//! Supported range, Dictionary membership, Corpus frequency, then each Profile's Answer list and
//! Guess pool at that language and length.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal as _;

use lex_core::{Strategy, answer_prior};
use lex_data::{
    DataDir, Language, MAX_WORD_LENGTH, MIN_SUPPORTED_WORDS, MIN_WORD_LENGTH, Profile,
    ProfileWords, Word, WordSet,
};
use strum::VariantArray as _;

use crate::cli::WordArgs;
use crate::corpora::not_cached_hint as corpus_hint;
use crate::dicts::{flag, not_cached_hint as dict_hint, render_rows, row};

/// The label every Prior carries: it is the one the default Strategy derives.
const STRATEGY_LABEL: &str = "(default Strategy)";

/// A cell for a step that was not reached.
const SKIPPED: &str = "-";

/// A cell for a step that needs a cache that is not there.
const UNKNOWN: &str = "unknown";

/// Whether a word is in the cached Dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dictionary {
    /// The word never reached this step.
    Skipped,
    /// The language's Dictionary is not cached.
    NotCached,
    /// The Dictionary holds the word.
    Member,
    /// The Dictionary does not hold the word.
    Missing,
}

/// What the cached Corpus says of a word.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Corpus {
    /// The word never reached this step.
    Skipped,
    /// The Corpus of the word's length is not cached.
    NotCached,
    /// The Corpus holds fewer than the least words a Supported length needs.
    Unsupported {
        /// Words the Corpus holds at this length.
        count: usize,
    },
    /// A Supported Corpus that never saw the word.
    Unseen,
    /// A Supported Corpus that holds the word.
    Seen {
        /// Position by frequency, from 1.
        rank: usize,
        /// Words in the Corpus.
        size: usize,
        /// Summed ngrams count.
        frequency: u64,
        /// Probability of being the answer at game start, over the Corpus.
        prior: f64,
    },
}

/// Where a word stands in one Profile's lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileStatus {
    /// The Profile is not fetched.
    NotFetched,
    /// On the Answer list.
    Answer {
        /// Probability of being the answer at game start, over the Answer list; unknown when
        /// the Corpus is not cached.
        prior: Option<f64>,
        /// Words on the Answer list.
        answers: usize,
    },
    /// In the Guess pool only.
    GuessOnly,
    /// In neither list.
    Outside,
}

/// A Profile at the word's language and length, and where the word stands in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileFact {
    /// Which Profile.
    pub profile: Profile,
    /// The word's standing.
    pub status: ProfileStatus,
}

/// Everything known about one word.
#[derive(Debug, Clone, PartialEq)]
pub struct Facts {
    /// The word as typed.
    pub input: String,
    /// The word after NFC and lowercasing.
    pub word: String,
    /// Letters in the normalized word.
    pub length: usize,
    /// One alphabetic token.
    pub shape: bool,
    /// The length is within `MIN_WORD_LENGTH..=MAX_WORD_LENGTH`.
    pub in_range: bool,
    /// Dictionary membership.
    pub dictionary: Dictionary,
    /// What the Corpus says.
    pub corpus: Corpus,
    /// Each Profile at this language and length.
    pub profiles: Vec<ProfileFact>,
}

impl Facts {
    /// A word before any lookup: shape and range decided, everything else not reached.
    fn new(input: &str) -> Self {
        let word = lex_data::normalize(input);
        let length = word.chars().count();
        Self {
            shape: !word.is_empty() && lex_data::is_valid_word(&word),
            in_range: (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&length),
            input: input.to_owned(),
            word,
            length,
            dictionary: Dictionary::Skipped,
            corpus: Corpus::Skipped,
            profiles: Vec::new(),
        }
    }

    /// Whether the lookups apply: a word of one alphabetic token in the Supported range.
    const fn reaches_caches(&self) -> bool {
        self.shape && self.in_range
    }
}

/// The Prior's temperature: the default Strategy's. Its shape is `Power`, which is what
/// `answer_prior` computes.
const fn temperature() -> f64 {
    Strategy::DEFAULT.temperature
}

/// What one Corpus holds of the words in it.
struct CorpusIndex<const N: usize> {
    /// Words in the Corpus.
    size: usize,
    /// Each word's rank (from 1), frequency and Prior.
    entries: HashMap<Word<N>, (usize, u64, f64)>,
    /// The least frequency in the Corpus, which a Profile word it lacks takes.
    floor: u64,
}

impl<const N: usize> CorpusIndex<N> {
    /// Indexes `corpus`; the Prior is the solver's own, at the default Strategy.
    fn new(corpus: &WordSet<N>) -> Self {
        let priors = answer_prior(corpus, temperature());
        let entries = corpus
            .word_frequencies()
            .zip(priors)
            .enumerate()
            .map(|(i, ((word, frequency), (_, prior)))| (word, (i + 1, frequency, prior)))
            .collect();
        let floor = corpus.word_frequencies().last().map_or(1, |(_, f)| f);
        Self {
            size: corpus.len(),
            entries,
            floor,
        }
    }
}

/// One Profile's lists as words of length `N`, with the Answer list's Prior.
struct ProfileLists<const N: usize> {
    /// Which Profile.
    profile: Profile,
    /// The lists, or `None` when the Profile is not fetched.
    lists: Option<Lists<N>>,
}

/// A fetched Profile's lists.
struct Lists<const N: usize> {
    /// The Answer list.
    answers: HashSet<Word<N>>,
    /// The guess-only words.
    guess_only: HashSet<Word<N>>,
    /// The Prior of each answer over the Answer list; empty when the Corpus is not cached.
    prior: HashMap<Word<N>, f64>,
}

impl<const N: usize> ProfileLists<N> {
    /// Reads `profile`'s lists; the Prior takes Corpus frequencies, the floor for a word the
    /// Corpus lacks, as the Profile's Guess pool does.
    fn load(
        data: &DataDir,
        profile: Profile,
        index: Option<&CorpusIndex<N>>,
    ) -> anyhow::Result<Self> {
        let lists = data
            .profile_words(profile)?
            .map(|words| Lists::new(&words, index));
        Ok(Self { profile, lists })
    }

    /// Where `word` stands.
    fn status(&self, word: &Word<N>) -> ProfileStatus {
        let Some(lists) = &self.lists else {
            return ProfileStatus::NotFetched;
        };
        if lists.answers.contains(word) {
            ProfileStatus::Answer {
                prior: lists.prior.get(word).copied(),
                answers: lists.answers.len(),
            }
        } else if lists.guess_only.contains(word) {
            ProfileStatus::GuessOnly
        } else {
            ProfileStatus::Outside
        }
    }
}

impl<const N: usize> Lists<N> {
    /// The lists as words of length `N`; a stored word of another length cannot occur in a
    /// verified list and is dropped.
    fn new(words: &ProfileWords, index: Option<&CorpusIndex<N>>) -> Self {
        let parse = |list: &[String]| -> Vec<Word<N>> {
            list.iter()
                .filter_map(|w| Word::<N>::try_from(w.as_str()).ok())
                .collect()
        };
        let answers = parse(&words.answers);
        let guess_only = parse(&words.guess_only);
        let prior = index.map_or_else(HashMap::new, |index| {
            let pairs = answers
                .iter()
                .map(|w| (*w, index.entries.get(w).map_or(index.floor, |e| e.1)))
                .collect();
            answer_prior(&WordSet::from_pairs(pairs), temperature())
                .into_iter()
                .collect()
        });
        Self {
            answers: answers.into_iter().collect(),
            guess_only: guess_only.into_iter().collect(),
            prior,
        }
    }
}

/// The words of one length to look up, with where they sit in the result.
struct Batch<'a> {
    /// The cache.
    data: &'a DataDir,
    /// The language.
    lang: Language,
    /// Facts to fill.
    facts: Vec<&'a mut Facts>,
}

/// What the Corpus says of `word`.
fn corpus_fact<const N: usize>(
    corpus: Option<&WordSet<N>>,
    index: Option<&CorpusIndex<N>>,
    word: &Word<N>,
) -> Corpus {
    match (corpus, index) {
        (Some(set), _) if set.len() < MIN_SUPPORTED_WORDS => {
            Corpus::Unsupported { count: set.len() }
        }
        (Some(_), Some(index)) => {
            index
                .entries
                .get(word)
                .map_or(Corpus::Unseen, |&(rank, frequency, prior)| Corpus::Seen {
                    rank,
                    size: index.size,
                    frequency,
                    prior,
                })
        }
        _ => Corpus::NotCached,
    }
}

/// Fills the Corpus and Profile facts of a batch of words of length `N`.
fn fill_length<const N: usize>(batch: Batch<'_>) -> anyhow::Result<()> {
    let Batch { data, lang, facts } = batch;
    let corpus = data.cached_corpus::<N>(lang)?;
    let index = corpus.as_ref().map(CorpusIndex::new);
    let profiles = Profile::VARIANTS
        .iter()
        .filter(|p| p.spec().language == lang && p.spec().word_length == N)
        .map(|&p| ProfileLists::<N>::load(data, p, index.as_ref()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    for fact in facts {
        let Ok(word) = Word::<N>::try_from(fact.word.as_str()) else {
            continue;
        };
        fact.corpus = corpus_fact(corpus.as_ref(), index.as_ref(), &word);
        fact.profiles = profiles
            .iter()
            .map(|p| ProfileFact {
                profile: p.profile,
                status: p.status(&word),
            })
            .collect();
    }
    Ok(())
}

/// Looks every word of `inputs` up in the caches of `lang`.
///
/// # Errors
///
/// If a cached file exists but cannot be read, or a Corpus is damaged.
pub fn look_up(data: &DataDir, lang: Language, inputs: &[String]) -> anyhow::Result<Vec<Facts>> {
    let mut facts: Vec<Facts> = inputs.iter().map(|w| Facts::new(w)).collect();
    let wanted: HashSet<String> = facts
        .iter()
        .filter(|f| f.reaches_caches())
        .map(|f| f.word.clone())
        .collect();
    if wanted.is_empty() {
        return Ok(facts);
    }
    let members = data.dict_members(lang, &wanted)?;
    let mut by_length: BTreeMap<usize, Vec<&mut Facts>> = BTreeMap::new();
    for fact in facts.iter_mut().filter(|f| f.reaches_caches()) {
        fact.dictionary = match &members {
            None => Dictionary::NotCached,
            Some(found) if found.contains(&fact.word) => Dictionary::Member,
            Some(_) => Dictionary::Missing,
        };
        by_length.entry(fact.length).or_default().push(fact);
    }
    for (length, group) in by_length {
        let batch = Batch {
            data,
            lang,
            facts: group,
        };
        lex_data::match_word_length!(fill_length, length, batch)?;
    }
    Ok(facts)
}

/// `lex word`: looks the words up and prints the report.
///
/// # Errors
///
/// If a cached file cannot be read.
pub fn run(args: &WordArgs) -> anyhow::Result<()> {
    let data = DataDir::new(&args.data.data_dir);
    let facts = look_up(&data, args.lang, &args.words)?;
    let terminal = std::io::stdout().is_terminal();
    anstream::print!("{}", render(&facts, args.lang, terminal));
    Ok(())
}

/// The report: the step-by-step view of a single word on a terminal, otherwise a row per word.
#[must_use]
pub fn render(facts: &[Facts], lang: Language, terminal: bool) -> String {
    match facts {
        [only] if terminal => render_steps(only, lang, terminal),
        _ => render_table(facts, lang, terminal),
    }
}

/// A Prior as a short scientific number.
fn prior_cell(prior: f64) -> String {
    format!("{prior:.3e}")
}

/// The input column: what was typed, empty when it is already the normalized form.
fn input_cell(facts: &Facts) -> String {
    if facts.input == facts.word {
        String::new()
    } else {
        facts.input.clone()
    }
}

/// `facts`' verdict: the first failing step, or that it is a Candidate in Corpus runs.
#[must_use]
pub fn verdict(facts: &Facts, lang: Language) -> String {
    if !facts.shape {
        return "not a word: not a single alphabetic token".to_owned();
    }
    if !facts.in_range {
        return format!(
            "length {} is outside the Supported range {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}",
            facts.length
        );
    }
    match (facts.corpus, facts.dictionary) {
        (Corpus::Unsupported { count }, _) => format!(
            "length {} is not Supported in {lang}: its Corpus holds {count} words, under {MIN_SUPPORTED_WORDS}",
            facts.length
        ),
        (_, Dictionary::Missing) => format!("not in the {lang} Dictionary"),
        (Corpus::Seen { rank, size, .. }, _) => {
            format!("a Candidate in Corpus runs (rank {rank} of {size})")
        }
        (Corpus::Unseen, Dictionary::Member) => {
            "in the Dictionary, but the ngrams snapshot never saw it".to_owned()
        }
        (Corpus::Unseen, _) => {
            "the ngrams snapshot never saw it (the Dictionary is not cached)".to_owned()
        }
        (_, Dictionary::NotCached) => {
            "unknown: the Dictionary and the Corpus are not cached".to_owned()
        }
        (_, _) => "unknown: the Corpus is not cached".to_owned(),
    }
}

/// The Dictionary column.
const fn dictionary_cell(dictionary: Dictionary) -> &'static str {
    match dictionary {
        Dictionary::Skipped => SKIPPED,
        Dictionary::NotCached => "not cached",
        Dictionary::Member => "yes",
        Dictionary::Missing => "no",
    }
}

/// The rank, frequency and Prior columns.
fn corpus_cells(corpus: Corpus) -> [String; 3] {
    match corpus {
        Corpus::Skipped | Corpus::Unsupported { .. } => [SKIPPED; 3].map(str::to_owned),
        Corpus::NotCached => [UNKNOWN; 3].map(str::to_owned),
        Corpus::Unseen => ["unseen", "0", SKIPPED].map(str::to_owned),
        Corpus::Seen {
            rank,
            size,
            frequency,
            prior,
        } => [
            format!("{rank}/{size}"),
            frequency.to_string(),
            prior_cell(prior),
        ],
    }
}

/// One Profile's status as a short cell.
fn profile_cell(fact: &ProfileFact) -> String {
    let status = match fact.status {
        ProfileStatus::NotFetched => "not fetched".to_owned(),
        ProfileStatus::Answer { prior: None, .. } => "answer".to_owned(),
        ProfileStatus::Answer {
            prior: Some(prior), ..
        } => format!("answer {}", prior_cell(prior)),
        ProfileStatus::GuessOnly => "guess-only".to_owned(),
        ProfileStatus::Outside => "no".to_owned(),
    };
    format!("{}: {status}", fact.profile)
}

/// A row per word, as a table (`terminal`) or tab-separated lines.
fn render_table(facts: &[Facts], lang: Language, terminal: bool) -> String {
    let prior_header = format!("prior {STRATEGY_LABEL}");
    let header = [
        "word",
        "input",
        "length",
        "dictionary",
        "rank",
        "frequency",
        prior_header.as_str(),
        "profiles",
        "verdict",
    ];
    let rows = facts
        .iter()
        .map(|f| {
            let [rank, frequency, prior] = corpus_cells(f.corpus);
            let profiles = if f.profiles.is_empty() {
                SKIPPED.to_owned()
            } else {
                f.profiles
                    .iter()
                    .map(profile_cell)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            vec![
                f.word.clone(),
                input_cell(f),
                f.length.to_string(),
                dictionary_cell(f.dictionary).to_owned(),
                rank,
                frequency,
                prior,
                profiles,
                verdict(f, lang),
            ]
        })
        .collect();
    render_rows(&header, rows, terminal)
}

/// The step-by-step report of one word, as `step`, `result` and `note` rows.
fn render_steps(facts: &Facts, lang: Language, terminal: bool) -> String {
    let mut rows = Vec::new();
    if facts.input != facts.word {
        rows.push(row("input", &facts.input, "as typed"));
    }
    rows.extend([
        row("word", &facts.word, ""),
        row(
            "shape",
            yes_no(facts.shape, terminal),
            "one alphabetic token after NFC and lowercasing",
        ),
    ]);
    if facts.shape {
        rows.push(length_row(facts, lang, terminal));
    }
    if facts.reaches_caches() {
        rows.extend(corpus_rows(facts, lang, terminal));
        rows.extend(
            facts
                .profiles
                .iter()
                .flat_map(|p| profile_rows(p, terminal)),
        );
    }
    rows.push(row("verdict", verdict(facts, lang), ""));
    render_rows(&["step", "result", "note"], rows, terminal)
}

/// `yes`, or `no` in the failure style.
fn yes_no(value: bool, terminal: bool) -> String {
    if value {
        "yes".to_owned()
    } else {
        flag("no", terminal)
    }
}

/// The length row: in range, and Supported when the Corpus says so.
fn length_row(facts: &Facts, lang: Language, terminal: bool) -> Vec<String> {
    let note = if facts.in_range {
        match facts.corpus {
            Corpus::Unsupported { count } => flag(
                &format!(
                    "not Supported in {lang}: the Corpus holds {count} words, under {MIN_SUPPORTED_WORDS}"
                ),
                terminal,
            ),
            Corpus::NotCached => format!("Supported unknown: {}", corpus_hint(lang, facts.length)),
            Corpus::Skipped | Corpus::Unseen | Corpus::Seen { .. } => {
                "in range and Supported".to_owned()
            }
        }
    } else {
        flag(
            &format!("outside the Supported range {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}"),
            terminal,
        )
    };
    row("length", facts.length.to_string(), note)
}

/// The Dictionary, Corpus, frequency and Prior rows.
fn corpus_rows(facts: &Facts, lang: Language, terminal: bool) -> Vec<Vec<String>> {
    let dictionary_note = match facts.dictionary {
        Dictionary::NotCached => dict_hint(lang),
        Dictionary::Skipped | Dictionary::Member | Dictionary::Missing => String::new(),
    };
    let dictionary = match facts.dictionary {
        Dictionary::Missing => flag("no", terminal),
        Dictionary::Skipped | Dictionary::NotCached | Dictionary::Member => {
            dictionary_cell(facts.dictionary).to_owned()
        }
    };
    let mut rows = vec![row("dictionary", dictionary, dictionary_note)];
    let prior_label = format!("prior {STRATEGY_LABEL}");
    match facts.corpus {
        Corpus::Skipped | Corpus::Unsupported { .. } => {}
        Corpus::NotCached => {
            rows.push(row("corpus", "not cached", corpus_hint(lang, facts.length)));
            rows.push(row(&prior_label, UNKNOWN, ""));
        }
        Corpus::Unseen => {
            let note = match facts.dictionary {
                Dictionary::Member => "in the Dictionary, but the ngrams snapshot never saw it",
                Dictionary::Missing | Dictionary::NotCached | Dictionary::Skipped => {
                    "the ngrams snapshot never saw it"
                }
            };
            rows.push(row("corpus", flag("not in the Corpus", terminal), note));
        }
        Corpus::Seen {
            rank,
            size,
            frequency,
            prior,
        } => {
            rows.push(row("corpus", format!("rank {rank} of {size}"), ""));
            rows.push(row(
                "frequency",
                frequency.to_string(),
                "summed ngrams count",
            ));
            rows.push(row(
                &prior_label,
                prior_cell(prior),
                format!("over the {lang} Corpus of {size} words"),
            ));
        }
    }
    rows
}

/// The Answer list and Guess pool rows of one Profile.
fn profile_rows(fact: &ProfileFact, terminal: bool) -> Vec<Vec<String>> {
    let name = fact.profile;
    let answers = format!("{name} answer list");
    let pool = format!("{name} guess pool");
    match fact.status {
        ProfileStatus::NotFetched => vec![row(
            format!("{name} lists"),
            "not fetched",
            "fetched on first play",
        )],
        ProfileStatus::Answer { prior, answers: n } => vec![
            row(
                answers,
                "yes",
                prior.map_or_else(
                    || format!("prior unknown: the Corpus is not cached {STRATEGY_LABEL}"),
                    |p| format!("prior {} over {n} answers {STRATEGY_LABEL}", prior_cell(p)),
                ),
            ),
            row(pool, "yes", ""),
        ],
        ProfileStatus::GuessOnly => vec![
            row(answers, yes_no(false, terminal), ""),
            row(pool, "yes", "guess-only"),
        ],
        ProfileStatus::Outside => vec![
            row(answers, yes_no(false, terminal), ""),
            row(pool, yes_no(false, terminal), ""),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lex_core::PriorShape;

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
        assert!(steps.contains("no english Dictionary is cached; it is fetched on first use"));
        assert!(steps.contains("no english Corpus of length 5 is cached"));
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
}
