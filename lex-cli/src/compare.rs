//! `lex compare`: paired comparison of two simulation reports.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;

use anyhow::Context;
use tabled::builder::Builder;
use tabled::settings::Style;

use crate::report::{Game, MAX_GUESSES, Report};
use crate::style;

/// Prints the paired comparison of the two reports `args` names.
///
/// # Errors
///
/// If either report cannot be read or parsed.
pub fn compare_reports(args: &crate::cli::CompareArgs) -> anyhow::Result<()> {
    let baseline = load(&args.baseline)?;
    let candidate = load(&args.candidate)?;
    let comparison = comparison(&baseline, &candidate)?;
    if std::io::stdout().is_terminal() {
        anstream::print!("{}", comparison.render_table());
    } else {
        print!("{}", comparison.render_plain());
    }
    Ok(())
}

/// One measured line of the comparison.
struct Metric {
    /// The line as scripts read it when output is piped: `benchmarks/scripts/prior-screen.sh`
    /// greps `mean guesses`, so these texts are an interface.
    plain: String,
    /// The measure's name in the terminal table.
    label: &'static str,
    /// Its value in the terminal table.
    value: String,
    /// How the value is styled in the terminal table.
    style: anstyle::Style,
}

/// A paired comparison, ready to print as plain lines or as a terminal table.
pub struct Comparison {
    /// Warnings, differing strategies and the paired-target count, one per line.
    header: Vec<String>,
    /// The measures; empty when no target was played by both reports.
    metrics: Vec<Metric>,
}

impl Comparison {
    /// Plain lines with no styling and no borders, the form scripts parse.
    #[must_use]
    pub fn render_plain(&self) -> String {
        let mut out = String::new();
        for line in self
            .header
            .iter()
            .chain(self.metrics.iter().map(|m| &m.plain))
        {
            let _ = writeln!(out, "{line}");
        }
        out
    }

    /// The header lines, then the measures as a bordered, coloured table.
    #[must_use]
    pub fn render_table(&self) -> String {
        let mut out = String::new();
        for line in &self.header {
            let style = if line.starts_with("WARNING") {
                style::failure_style()
            } else {
                style::dim_style()
            };
            let _ = writeln!(out, "{style}{line}{style:#}");
        }
        if self.metrics.is_empty() {
            return out;
        }
        let mut builder = Builder::new();
        builder.push_record(["Measure", "Value"]);
        for m in &self.metrics {
            builder.push_record([
                m.label.to_owned(),
                format!("{}{}{:#}", m.style, m.value, m.style),
            ]);
        }
        let mut table = builder.build();
        table.with(Style::rounded());
        let _ = writeln!(out, "{table}");
        out
    }
}

/// Reads the JSON report at `path`.
fn load(path: &Path) -> anyhow::Result<Report> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading report {}", path.display()))?;
    serde_json::from_str(&content).with_context(|| format!("parsing report {}", path.display()))
}

/// Errors unless both reports played the same Profile, or neither played one.
fn check_profiles(baseline: &Report, candidate: &Report) -> anyhow::Result<()> {
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
    Ok(())
}

/// The measures over the targets both reports played; `pairs` is not empty.
fn metrics(pairs: &[(&Game, &Game)]) -> Vec<Metric> {
    let n: f64 = pairs.iter().map(|_| 1.0).sum();
    let diff = |(g, h): &(&Game, &Game)| f64::from(h.num_guesses) - f64::from(g.num_guesses);
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
    let fail = |games: &mut dyn Iterator<Item = u32>| {
        games.filter(|&k| k > MAX_GUESSES).map(|_| 1.0).sum::<f64>() / n
    };
    let base_fail = fail(&mut pairs.iter().map(|(g, _)| g.num_guesses));
    let cand_fail = fail(&mut pairs.iter().map(|(_, h)| h.num_guesses));
    let changed = pairs.iter().filter(|(g, h)| g.guesses != h.guesses).count();

    let unchanged = pairs.len() - better - worse;
    let delta_style = |d: f64| {
        if d < 0.0 {
            style::success_style()
        } else if d > 0.0 {
            style::failure_style()
        } else {
            anstyle::Style::new()
        }
    };
    vec![
        Metric {
            plain: format!("mean guesses (uniform):  {uniform_delta:+.4}  (negative is better)"),
            label: "mean guesses (uniform)",
            value: format!("{uniform_delta:+.4}"),
            style: delta_style(uniform_delta).bold(),
        },
        Metric {
            plain: format!("mean guesses (prior):    {prior_delta:+.4}"),
            label: "mean guesses (prior)",
            value: format!("{prior_delta:+.4}"),
            style: delta_style(prior_delta).bold(),
        },
        Metric {
            plain: format!(
                "failure rate (>{MAX_GUESSES}):      {:.2}% -> {:.2}%",
                base_fail * 100.0,
                cand_fail * 100.0
            ),
            label: "failure rate",
            value: format!("{:.2}% -> {:.2}%", base_fail * 100.0, cand_fail * 100.0),
            style: delta_style(cand_fail - base_fail),
        },
        Metric {
            plain: format!("targets: {better} better, {worse} worse, {unchanged} unchanged"),
            label: "targets",
            value: format!("{better} better, {worse} worse, {unchanged} unchanged"),
            style: anstyle::Style::new(),
        },
        Metric {
            plain: format!("guess sequences changed: {changed}"),
            label: "guess sequences changed",
            value: changed.to_string(),
            style: anstyle::Style::new(),
        },
    ]
}

/// Paired comparison over the targets both reports played. Pairing removes target-sampling
/// noise, so a small change is visible without a huge sample.
///
/// # Errors
///
/// If the reports played different Profiles, or only one played a Profile: their targets and
/// priors are not comparable.
pub fn compare(baseline: &Report, candidate: &Report) -> anyhow::Result<String> {
    Ok(comparison(baseline, candidate)?.render_plain())
}

/// The paired comparison as data; [`compare`] is its plain-text form.
///
/// # Errors
///
/// As [`compare`].
pub fn comparison(baseline: &Report, candidate: &Report) -> anyhow::Result<Comparison> {
    check_profiles(baseline, candidate)?;
    let mut header = Vec::new();
    let (b, c) = (&baseline.corpus, &candidate.corpus);
    if (&b.language, b.word_length) != (&c.language, c.word_length) {
        header.push(format!(
            "WARNING: different (language, length): {} {} vs {} {}",
            b.language, b.word_length, c.language, c.word_length
        ));
    }
    if b.ngrams_sha256 != c.ngrams_sha256 {
        header.push(
            "WARNING: corpus checksums differ; differences may come from data, not the solver"
                .to_owned(),
        );
    }

    if baseline.run.strategy != candidate.run.strategy {
        header.push(format!(
            "strategy: {} -> {}",
            baseline.run.strategy, candidate.run.strategy
        ));
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
    header.push(format!(
        "paired targets: {} (baseline {}, candidate {})",
        pairs.len(),
        baseline.games.len(),
        candidate.games.len()
    ));
    if pairs.is_empty() {
        return Ok(Comparison {
            header,
            metrics: Vec::new(),
        });
    }

    let metrics = metrics(&pairs);
    Ok(Comparison { header, metrics })
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

    fn with_games(counts: &[u32]) -> Report {
        let mut r = report(None);
        r.games = counts
            .iter()
            .enumerate()
            .map(|(i, &k)| crate::report::Game {
                word: format!("w{i}"),
                num_guesses: k,
                prior: 0.25,
                guesses: vec!["a".to_owned(); usize::try_from(k).unwrap()],
            })
            .collect();
        r
    }

    #[test]
    fn plain_output_keeps_the_lines_scripts_grep() {
        let plain = compare(&with_games(&[4, 5, 7, 3]), &with_games(&[3, 5, 6, 3])).unwrap();
        assert!(plain.contains("\nmean guesses (uniform):  -0.5000  (negative is better)\n"));
        assert!(plain.contains("\nmean guesses (prior):    -0.5000\n"));
        assert!(plain.contains("\ntargets: 2 better, 0 worse, 2 unchanged\n"));
        assert!(!plain.contains('\u{1b}'));
    }

    #[test]
    fn the_table_holds_the_same_measures() {
        let c = comparison(&with_games(&[4, 5]), &with_games(&[5, 5])).unwrap();
        let table = c.render_table();
        assert!(table.contains("mean guesses (uniform)"));
        assert!(table.contains("+0.5000"));
        assert!(table.contains('\u{256d}'));
    }
}
