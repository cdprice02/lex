//! `lex compare`: paired comparison of two simulation reports.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context;

use crate::report::{MAX_GUESSES, Report};

/// Prints the paired comparison of the two reports `args` names.
///
/// # Errors
///
/// If either report cannot be read or parsed.
pub fn compare_reports(args: &crate::cli::CompareArgs) -> anyhow::Result<()> {
    let baseline = load(&args.baseline)?;
    let candidate = load(&args.candidate)?;
    print!("{}", compare(&baseline, &candidate)?);
    Ok(())
}

/// Reads the JSON report at `path`.
fn load(path: &Path) -> anyhow::Result<Report> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading report {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing report {}", path.display()))
}

/// Paired comparison over the targets both reports played. Pairing removes target-sampling
/// noise, so a small change is visible without a huge sample.
///
/// # Errors
///
/// If the reports played different Profiles, or only one played a Profile: their targets and
/// priors are not comparable.
pub fn compare(baseline: &Report, candidate: &Report) -> anyhow::Result<String> {
    match (&baseline.profile, &candidate.profile) {
        (None, None) => {}
        (Some(b), Some(c)) if b.same_lists(c) => {}
        (b, c) => {
            let name = |p: &Option<crate::report::ProfileReport>| {
                p.as_ref()
                    .map_or("no profile", |p| p.name.as_str())
                    .to_owned()
            };
            anyhow::bail!(
                "reports played different profiles: {} vs {}",
                name(b),
                name(c)
            );
        }
    }
    let mut out = String::new();
    let (b, c) = (&baseline.corpus, &candidate.corpus);
    if (&b.language, b.word_length) != (&c.language, c.word_length) {
        let _ = writeln!(
            out,
            "WARNING: different (language, length): {} {} vs {} {}",
            b.language, b.word_length, c.language, c.word_length
        );
    }
    if b.ngrams_sha256 != c.ngrams_sha256 {
        let _ = writeln!(
            out,
            "WARNING: corpus checksums differ; differences may come from data, not the solver"
        );
    }

    if baseline.run.strategy != candidate.run.strategy {
        let _ = writeln!(
            out,
            "strategy: {} -> {}",
            baseline.run.strategy, candidate.run.strategy
        );
    }

    let candidate_by_word: HashMap<&str, _> = candidate
        .games
        .iter()
        .map(|g| (g.word.as_str(), g))
        .collect();
    let pairs: Vec<_> = baseline
        .games
        .iter()
        .filter_map(|g| candidate_by_word.get(g.word.as_str()).map(|&h| (g, h)))
        .collect();
    let _ = writeln!(
        out,
        "paired targets: {} (baseline {}, candidate {})",
        pairs.len(),
        baseline.games.len(),
        candidate.games.len()
    );
    if pairs.is_empty() {
        return Ok(out);
    }

    let n = crate::cast::count(pairs.len());
    let diff = |(g, h): &(&crate::report::Game, &crate::report::Game)| {
        crate::cast::count(h.num_guesses) - crate::cast::count(g.num_guesses)
    };
    let uniform_delta = pairs.iter().map(diff).sum::<f64>() / n;
    let total_prior: f64 = pairs.iter().map(|(g, _)| g.prior).sum();
    let prior_delta = pairs.iter().map(|p| p.0.prior * diff(p)).sum::<f64>() / total_prior;
    let better = pairs
        .iter()
        .filter(|(g, h)| h.num_guesses < g.num_guesses)
        .count();
    let worse = pairs
        .iter()
        .filter(|(g, h)| h.num_guesses > g.num_guesses)
        .count();
    let fail = |games: &mut dyn Iterator<Item = usize>| {
        crate::cast::count(games.filter(|&k| k > MAX_GUESSES).count()) / n
    };
    let base_fail = fail(&mut pairs.iter().map(|(g, _)| g.num_guesses));
    let cand_fail = fail(&mut pairs.iter().map(|(_, h)| h.num_guesses));
    let changed = pairs.iter().filter(|(g, h)| g.guesses != h.guesses).count();

    let _ = writeln!(
        out,
        "mean guesses (uniform):  {uniform_delta:+.4}  (negative is better)"
    );
    let _ = writeln!(out, "mean guesses (prior):    {prior_delta:+.4}");
    let _ = writeln!(
        out,
        "failure rate (>{MAX_GUESSES}):      {:.2}% -> {:.2}%",
        base_fail * 100.0,
        cand_fail * 100.0
    );
    let _ = writeln!(
        out,
        "targets: {better} better, {worse} worse, {} unchanged",
        pairs.len() - better - worse
    );
    let _ = writeln!(out, "guess sequences changed: {changed}");
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{Corpus, ProfileReport, Report, Run, SCHEMA_VERSION, Summary};
    use lex_core::Strategy;

    fn profile(name: &str, answers: &str) -> ProfileReport {
        ProfileReport {
            name: name.to_owned(),
            answers_sha256: answers.to_owned(),
            guess_only_sha256: "g".to_owned(),
            answer_count: 1,
            guess_count: 2,
            floor: 1,
            floored: 0,
        }
    }

    fn report(profile: Option<ProfileReport>) -> Report {
        Report {
            schema_version: SCHEMA_VERSION,
            run: Run {
                lex_version: String::new(),
                timestamp: String::new(),
                seed: 0,
                num_games: 0,
                targets_file: None,
                first_guess: String::new(),
                strategy: Strategy::default().into(),
            },
            corpus: Corpus {
                language: "english".to_owned(),
                word_length: 5,
                dictionary_length: 0,
                ngrams_snapshot: String::new(),
                ngrams_sha256: String::new(),
                dict: None,
            },
            profile,
            summary: Summary::from_games(&[]),
            games: Vec::new(),
        }
    }

    #[test]
    fn matching_profiles_or_none_compare() {
        assert!(compare(&report(None), &report(None)).is_ok());
        let p = Some(profile("wordle", "a"));
        assert!(compare(&report(p.clone()), &report(p)).is_ok());
    }

    #[test]
    fn differing_or_one_sided_profiles_are_an_error() {
        let wordle = Some(profile("wordle", "a"));
        assert!(compare(&report(wordle.clone()), &report(None)).is_err());
        assert!(compare(&report(None), &report(wordle.clone())).is_err());
        let other_name = Some(profile("wordle-all", "a"));
        assert!(compare(&report(wordle.clone()), &report(other_name)).is_err());
        let other_lists = Some(profile("wordle", "b"));
        assert!(compare(&report(wordle), &report(other_lists)).is_err());
    }

    #[test]
    fn a_report_without_a_profile_field_reads_as_none() {
        let mut json = serde_json::to_value(report(None)).unwrap();
        json.as_object_mut().unwrap().remove("profile");
        let parsed: Report = serde_json::from_value(json).unwrap();
        assert!(parsed.profile.is_none());
    }
}
