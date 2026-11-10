//! `lex dict fetch`, `lex corpus build` and `lex profile fetch`: ready the caches ahead of time, so
//! later runs need no network (the `cargo fetch` model).
//!
//! Each command readies what it stands on: a Corpus needs its Dictionary, a Profile's lists need
//! its language's Corpus. A cached item is reported ready and left alone; `--force` clears it
//! (with the cascade of the matching `clear`) and redoes the work. Status lines and the closing
//! summary, which has the form of the matching `show`, go to the output writer; warnings go to
//! the other. Downloads go through [`Network`], so tests run the commands without one.

use std::io::{IsTerminal as _, Write};

use lex_data::{DataDir, Language, Profile};
use strum::VariantArray;

use crate::cli::{CorpusBuildArgs, DictFetchArgs, ProfileFetchArgs};
use crate::progress::Task;
use crate::{corpora, dicts, profiles};

/// What the commands download. [`Online`] is the real thing.
pub trait Network {
    /// Downloads the Dictionary of `lang`.
    ///
    /// # Errors
    ///
    /// If the download or the cache files fail.
    fn dict(&self, dir: &DataDir, lang: Language) -> anyhow::Result<()>;

    /// Builds the Corpus of `lang` at every length from its cached Dictionary, calling
    /// `progress(done, total)` as shards finish.
    ///
    /// # Errors
    ///
    /// If a download or a cache file fails.
    fn corpus(
        &self,
        dir: &DataDir,
        lang: Language,
        progress: &(dyn Fn(u32, u32) + Sync),
    ) -> anyhow::Result<()>;

    /// Fetches the lists of `profile` and verifies them against its compiled-in checksums. Its
    /// Corpus is already cached.
    ///
    /// # Errors
    ///
    /// If a download fails or a list does not verify.
    fn profile(&self, dir: &DataDir, profile: Profile) -> anyhow::Result<()>;
}

/// The real downloads, drawing their progress through [`crate::progress`].
pub struct Online;

impl Network for Online {
    fn dict(&self, dir: &DataDir, lang: Language) -> anyhow::Result<()> {
        let task = Task::start(&format!("fetching the {lang} Dictionary"));
        let result = dir.fetch_dict(lang);
        task.finish();
        result
    }

    fn corpus(
        &self,
        dir: &DataDir,
        lang: Language,
        progress: &(dyn Fn(u32, u32) + Sync),
    ) -> anyhow::Result<()> {
        dir.build_corpus(lang, progress)
    }

    fn profile(&self, dir: &DataDir, profile: Profile) -> anyhow::Result<()> {
        lex_data::match_word_length!(load_profile, profile.spec().word_length, &(dir, profile))
    }
}

/// Loads `profile` at length `N`, which fetches and verifies its lists when absent.
fn load_profile<const N: usize>((dir, profile): &(&DataDir, Profile)) -> anyhow::Result<()> {
    dir.load_profile::<N>(*profile).map(|_| ())
}

/// Where a command writes: `out` for status lines and summaries, `warn` for warnings.
pub struct Sinks<'a> {
    /// Status lines and summaries.
    pub out: &'a mut dyn Write,
    /// Warnings.
    pub warn: &'a mut dyn Write,
    /// Whether `out` is a terminal, which picks the table or tab-separated summary.
    pub terminal: bool,
}

/// `lex dict fetch`.
///
/// # Errors
///
/// If a download or a cache file fails.
pub fn dict(args: &DictFetchArgs) -> anyhow::Result<()> {
    let mut sinks = Sinks {
        out: &mut anstream::stdout(),
        warn: &mut anstream::stderr(),
        terminal: std::io::stdout().is_terminal(),
    };
    dict_with(args, &Online, &mut sinks)
}

/// `lex corpus build`.
///
/// # Errors
///
/// If a download or a cache file fails.
pub fn corpus(args: &CorpusBuildArgs) -> anyhow::Result<()> {
    let mut sinks = Sinks {
        out: &mut anstream::stdout(),
        warn: &mut anstream::stderr(),
        terminal: std::io::stdout().is_terminal(),
    };
    corpus_with(args, &Online, &mut sinks)
}

/// `lex profile fetch`.
///
/// # Errors
///
/// If a download or a cache file fails, or a list does not verify.
pub fn profile(args: &ProfileFetchArgs) -> anyhow::Result<()> {
    let mut sinks = Sinks {
        out: &mut anstream::stdout(),
        warn: &mut anstream::stderr(),
        terminal: std::io::stdout().is_terminal(),
    };
    profile_with(args, &Online, &mut sinks)
}

/// The languages `--lang` names, or every language with `--all`.
fn languages(lang: Option<Language>) -> Vec<Language> {
    lang.map_or_else(|| Language::VARIANTS.to_vec(), |lang| vec![lang])
}

/// A blank line between the summaries of consecutive items.
fn separate(sinks: &mut Sinks<'_>, index: usize) -> anyhow::Result<()> {
    if index > 0 {
        writeln!(sinks.out)?;
    }
    Ok(())
}

/// [`dict`] over `net`.
///
/// # Errors
///
/// As [`dict`].
pub fn dict_with(
    args: &DictFetchArgs,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    for (index, lang) in languages(args.lang).into_iter().enumerate() {
        separate(sinks, index)?;
        fetch_dict(&dir, lang, args.force, net, sinks)?;
    }
    Ok(())
}

/// Readies one Dictionary and prints it as `dict show` does.
fn fetch_dict(
    dir: &DataDir,
    lang: Language,
    force: bool,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    match dir.dict(lang)? {
        Some(record) if !force => {
            let fetched = record
                .metadata
                .as_ref()
                .and_then(|m| m.fetched_at())
                .map_or_else(|| "date unknown".to_owned(), dicts::fetch_date);
            writeln!(
                sinks.out,
                "the {lang} Dictionary is ready (fetched {fetched}); --force takes KAIKKI's current extract"
            )?;
        }
        _ => {
            if force {
                writeln!(
                    sinks.warn,
                    "warning: --force takes KAIKKI's current (rolling) Wiktionary extract, which can \
                     change the Corpus and the Track snapshots of {lang}; clearing the Corpus \
                     caches and books built from the old one"
                )?;
                dir.clear_dict(lang)?;
            }
            net.dict(dir, lang)?;
            writeln!(sinks.out, "fetched the {lang} Dictionary")?;
        }
    }
    let record = dir
        .dict(lang)?
        .ok_or_else(|| anyhow::anyhow!("the {lang} Dictionary is still absent after the fetch"))?;
    write!(sinks.out, "{}", dicts::render_show(&record, sinks.terminal))?;
    Ok(())
}

/// [`corpus`] over `net`.
///
/// # Errors
///
/// As [`corpus`].
pub fn corpus_with(
    args: &CorpusBuildArgs,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    for (index, lang) in languages(args.lang).into_iter().enumerate() {
        separate(sinks, index)?;
        let cached = cached_lengths(&dir, lang)?;
        if cached > 0 && !args.force {
            writeln!(
                sinks.out,
                "the {lang} Corpus is ready ({cached} lengths cached); --force rebuilds it"
            )?;
        } else {
            if args.force {
                dir.clear(lang, None)?;
            }
            build_corpus(&dir, lang, net, sinks)?;
        }
        let records: Vec<_> = dir
            .corpora()?
            .into_iter()
            .filter(|r| r.language == lang)
            .collect();
        write!(
            sinks.out,
            "{}",
            corpora::render_list(&records, sinks.terminal)
        )?;
    }
    Ok(())
}

/// How many lengths of `lang` have a Corpus on disk.
fn cached_lengths(dir: &DataDir, lang: Language) -> anyhow::Result<usize> {
    Ok(dir.corpora()?.iter().filter(|r| r.language == lang).count())
}

/// Builds the Corpus of `lang`, fetching its Dictionary first when absent.
fn build_corpus(
    dir: &DataDir,
    lang: Language,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    if !dir.dict_path(lang).exists() {
        writeln!(
            sinks.out,
            "the {lang} Corpus is filtered by its Dictionary, which is not cached"
        )?;
        net.dict(dir, lang)?;
        writeln!(sinks.out, "fetched the {lang} Dictionary")?;
    }
    let task = Task::start(&format!("building the {lang} Corpus"));
    let result = net.corpus(dir, lang, &|done, total| task.set(done, total));
    task.finish();
    result?;
    writeln!(sinks.out, "built the {lang} Corpus")?;
    Ok(())
}

/// [`profile`] over `net`.
///
/// # Errors
///
/// As [`profile`].
pub fn profile_with(
    args: &ProfileFetchArgs,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    let dir = DataDir::new(&args.data.data_dir);
    let chosen = args
        .profile
        .map_or_else(|| Profile::VARIANTS.to_vec(), |profile| vec![profile]);
    for (index, profile) in chosen.into_iter().enumerate() {
        separate(sinks, index)?;
        fetch_profile(&dir, profile, args.force, net, sinks)?;
    }
    Ok(())
}

/// Readies one Profile and prints it as `profile show` does.
fn fetch_profile(
    dir: &DataDir,
    profile: Profile,
    force: bool,
    net: &dyn Network,
    sinks: &mut Sinks<'_>,
) -> anyhow::Result<()> {
    match dir.fetched_profile(profile)? {
        Some(record) if !force => writeln!(
            sinks.out,
            "Profile {profile} is ready (fetched {}); --force fetches and verifies it again",
            dicts::fetch_date(&record.fetched_at)
        )?,
        _ => {
            if force {
                dir.clear_profile(profile)?;
            }
            let spec = profile.spec();
            let (lang, length) = (spec.language, spec.word_length);
            if !dir.ngrams_path(lang, length).is_file() {
                writeln!(
                    sinks.out,
                    "Profile {profile} loads the {lang} Corpus of length {length}, which is not cached"
                )?;
                build_corpus(dir, lang, net, sinks)?;
            }
            net.profile(dir, profile)?;
            writeln!(sinks.out, "fetched Profile {profile}")?;
        }
    }
    let record = dir
        .fetched_profile(profile)?
        .ok_or_else(|| anyhow::anyhow!("Profile {profile} is still not fetched after the fetch"))?;
    write!(
        sinks.out,
        "{}",
        profiles::render_show(&record, sinks.terminal)
    )?;
    Ok(())
}
