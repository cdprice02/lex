use lex_data::{DataDir, ProfileStats, Word, WordSet};

use crate::cli::CommonArgs;

/// What a game is played over, whichever source supplied it: the Guess pool, the Answer list
/// when a Profile sets one, and the Profile's identity for reports. A Corpus run has neither.
pub struct GameLists<const N: usize> {
    pub pool: WordSet<N>,
    pub answers: Option<Vec<Word<N>>>,
    pub profile: Option<ProfileStats>,
}

impl<const N: usize> GameLists<N> {
    /// Loads the Profile's lists when `--profile` is set, otherwise the Corpus (honouring
    /// `--dictionary-length`) as pool with no Answer list.
    pub fn load(args: &CommonArgs, data_dir: &DataDir) -> anyhow::Result<Self> {
        let (lang, _) = args.target()?;
        match args.profile {
            Some(profile) => {
                let data = data_dir.load_profile::<N>(profile)?;
                Ok(Self {
                    pool: data.pool,
                    answers: Some(data.answers),
                    profile: Some(data.stats),
                })
            }
            None => Ok(Self {
                pool: data_dir.load::<N>(lang, args.dictionary_length)?,
                answers: None,
                profile: None,
            }),
        }
    }

    /// The words a game can be about: the Answer list (alphabetical), else the whole pool in
    /// Corpus order.
    pub fn answer_words(&self) -> Vec<Word<N>> {
        match &self.answers {
            Some(answers) => answers.clone(),
            None => self.pool.words().collect(),
        }
    }

    /// Names the list targets must come from, for error messages.
    pub fn answer_noun(&self) -> String {
        match &self.profile {
            Some(p) => format!("the {} answer list", p.name),
            None => "the dictionary".to_owned(),
        }
    }
}
