use lex_data::{DataDir, Language, ProfileStats, Word, WordSet};

use crate::cli::CommonArgs;
use crate::report::{Corpus, DictProvenance, ProfileReport};

/// What a game is played over: the Guess pool and the Answer list, a subset of it. A Corpus run
/// answers with every pool word; a Profile with its own list. Where they came from is the
/// [`Source`], returned beside them.
pub struct GameLists<const N: usize> {
    pub pool: WordSet<N>,
    pub answers: Vec<Word<N>>,
}

/// Where the lists came from, with the statistics a report carries about it.
pub enum Source {
    Corpus(Corpus),
    Profile { corpus: Corpus, stats: ProfileStats },
}

impl Source {
    /// Loads the Profile's lists when `--profile` is set, otherwise the Corpus (honouring
    /// `--dictionary-length`) with every word as an answer.
    pub fn load<const N: usize>(
        args: &CommonArgs,
        data_dir: &DataDir,
    ) -> anyhow::Result<(GameLists<N>, Self)> {
        match args.profile {
            Some(profile) => {
                let lang = profile.spec().language;
                let data = data_dir.load_profile::<N>(profile)?;
                // The Corpus size, which the Profile's pool does not equal.
                let dictionary_length = data_dir.ngrams_count(lang, N)?;
                let corpus = corpus_report::<N>(data_dir, lang, dictionary_length)?;
                Ok((
                    GameLists {
                        pool: data.pool,
                        answers: data.answers,
                    },
                    Self::Profile {
                        corpus,
                        stats: data.stats,
                    },
                ))
            }
            None => {
                let lang = args.lang.expect("clap requires --lang without --profile");
                let pool = data_dir.load::<N>(lang, args.dictionary_length)?;
                let corpus = corpus_report::<N>(data_dir, lang, pool.len())?;
                let answers = pool.words().collect();
                Ok((GameLists { pool, answers }, Self::Corpus(corpus)))
            }
        }
    }

    /// The Corpus the lists were built from.
    pub fn corpus(&self) -> &Corpus {
        match self {
            Self::Corpus(corpus) | Self::Profile { corpus, .. } => corpus,
        }
    }

    /// The Corpus size, which is the pool size only on a Corpus run.
    pub fn dictionary_length(&self) -> usize {
        self.corpus().dictionary_length
    }

    /// The Profile's identity for a report; a Corpus run has none.
    pub fn profile_report(&self) -> Option<ProfileReport> {
        match self {
            Self::Corpus(_) => None,
            Self::Profile { stats, .. } => Some(stats.clone().into()),
        }
    }

    /// What the games are played in, for messages: the Profile's name or the language.
    pub fn name(&self) -> &str {
        match self {
            Self::Corpus(corpus) => &corpus.language,
            Self::Profile { stats, .. } => &stats.name,
        }
    }

    /// Names the list targets must come from, for error messages.
    pub fn answer_noun(&self) -> String {
        match self {
            Self::Corpus(_) => "the dictionary".to_owned(),
            Self::Profile { stats, .. } => format!("the {} answer list", stats.name),
        }
    }

    /// A Profile's Guess pool is closed; a Corpus run accepts any word, as it always has.
    pub fn check_guess<const N: usize>(
        &self,
        pool: &WordSet<N>,
        word: &Word<N>,
    ) -> Result<(), String> {
        match self {
            Self::Profile { stats, .. } if !pool.contains(word) => {
                Err(format!("{word} is not in the {} guess pool", stats.name))
            }
            _ => Ok(()),
        }
    }
}

fn corpus_report<const N: usize>(
    data_dir: &DataDir,
    lang: Language,
    dictionary_length: usize,
) -> anyhow::Result<Corpus> {
    let dict = match data_dir.dict_metadata(lang) {
        Ok(meta) => Some(DictProvenance {
            source_url: meta.source_url().map(str::to_owned),
            fetched_at: meta.fetched_at().map(str::to_owned),
            source_sha256: meta.source_sha256().map(str::to_owned),
            word_count: meta.word_count,
        }),
        Err(e) => {
            log::warn!("no dictionary metadata for the report: {e:#}");
            None
        }
    };
    Ok(Corpus {
        language: lang.to_string(),
        word_length: N,
        dictionary_length,
        ngrams_snapshot: lex_data::NGRAMS_SNAPSHOT.to_owned(),
        ngrams_sha256: data_dir.ngrams_sha256(lang, N)?,
        dict,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Corpus {
        Corpus {
            language: "english".to_owned(),
            word_length: 5,
            dictionary_length: 10,
            ngrams_snapshot: String::new(),
            ngrams_sha256: String::new(),
            dict: None,
        }
    }

    fn profile_source() -> Source {
        Source::Profile {
            corpus: corpus(),
            stats: ProfileStats {
                name: "wordle".to_owned(),
                answers_sha256: String::new(),
                guess_only_sha256: String::new(),
                answer_count: 1,
                guess_count: 1,
                floor: 1,
                floored: 0,
            },
        }
    }

    fn pool() -> WordSet<5> {
        WordSet::from_pairs(vec![(Word::try_from("crane").unwrap(), 1)])
    }

    #[test]
    fn a_profile_guess_pool_is_closed() {
        let (inside, outside) = (
            Word::try_from("crane").unwrap(),
            Word::try_from("zebra").unwrap(),
        );
        let source = profile_source();
        assert!(source.check_guess(&pool(), &inside).is_ok());
        assert_eq!(
            source.check_guess(&pool(), &outside).unwrap_err(),
            "zebra is not in the wordle guess pool"
        );
    }

    #[test]
    fn a_corpus_run_accepts_any_word() {
        let outside = Word::try_from("zebra").unwrap();
        assert!(
            Source::Corpus(corpus())
                .check_guess(&pool(), &outside)
                .is_ok()
        );
    }

    #[test]
    fn provenance_differs_by_variant() {
        let corpus_run = Source::Corpus(corpus());
        assert!(corpus_run.profile_report().is_none());
        assert_eq!(corpus_run.answer_noun(), "the dictionary");
        let profile_run = profile_source();
        assert_eq!(profile_run.profile_report().unwrap().name, "wordle");
        assert_eq!(profile_run.answer_noun(), "the wordle answer list");
        assert_eq!(profile_run.dictionary_length(), 10);
    }
}
