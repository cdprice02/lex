use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anyhow::Context;

use crate::report::{MAX_GUESSES, Report};

pub fn compare_reports(args: &crate::cli::CompareArgs) -> anyhow::Result<()> {
    let baseline = load(&args.baseline)?;
    let candidate = load(&args.candidate)?;
    print!("{}", compare(&baseline, &candidate));
    Ok(())
}

fn load(path: &Path) -> anyhow::Result<Report> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading report {path:?}"))?;
    serde_json::from_str(&content).with_context(|| format!("parsing report {path:?}"))
}

/// Paired comparison over the targets both reports played. Pairing removes target-sampling
/// noise, so a small change is visible without a huge sample.
pub fn compare(baseline: &Report, candidate: &Report) -> String {
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
        return out;
    }

    let n = pairs.len() as f64;
    let diff = |(g, h): &(&crate::report::Game, &crate::report::Game)| {
        h.num_guesses as f64 - g.num_guesses as f64
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
        games.filter(|&k| k > MAX_GUESSES).count() as f64 / n
    };
    let base_fail = fail(&mut pairs.iter().map(|(g, _)| g.num_guesses));
    let cand_fail = fail(&mut pairs.iter().map(|(_, h)| h.num_guesses));
    let changed = pairs.iter().filter(|(g, h)| g.guesses != h.guesses).count();

    let _ = writeln!(
        out,
        "mean guesses (uniform):  {:+.4}  (negative is better)",
        uniform_delta
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
    out
}
