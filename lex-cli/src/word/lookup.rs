//! The path itself: reading the Dictionary, Corpus and Profile caches into [`Facts`].

use std::collections::{BTreeMap, HashMap, HashSet};

use lex_core::{Strategy, answer_prior};
use lex_data::{DataDir, Language, MIN_SUPPORTED_WORDS, Profile, ProfileWords, Word, WordSet};
use strum::VariantArray as _;

use super::{Corpus, Dictionary, Facts, ProfileFact, ProfileStatus};

/// The Prior's temperature: the default Strategy's. Its shape is `Power`, which is what
/// `answer_prior` computes.
const fn temperature() -> f64 {
    Strategy::DEFAULT.temperature
}

/// What one Corpus holds of the words in it.
pub struct CorpusIndex<const N: usize> {
    /// Words in the Corpus.
    size: usize,
    /// Each word's rank (from 1), frequency and Prior.
    entries: HashMap<Word<N>, (usize, u64, f64)>,
    /// The least frequency in the Corpus, which a Profile word it lacks takes.
    pub floor: u64,
}

impl<const N: usize> CorpusIndex<N> {
    /// Indexes `corpus`; the Prior is the solver's own, at the default Strategy.
    pub fn new(corpus: &WordSet<N>) -> Self {
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
pub struct ProfileLists<const N: usize> {
    /// Which Profile.
    pub profile: Profile,
    /// The lists, or `None` when the Profile is not fetched.
    pub lists: Option<Lists<N>>,
}

/// A fetched Profile's lists.
pub struct Lists<const N: usize> {
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
    pub fn status(&self, word: &Word<N>) -> ProfileStatus {
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
    pub fn new(words: &ProfileWords, index: Option<&CorpusIndex<N>>) -> Self {
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
pub fn corpus_fact<const N: usize>(
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
