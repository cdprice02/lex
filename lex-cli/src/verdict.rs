//! `lex verdict`: judges a head set of Track snapshots against a base set by ADR 0007's rule,
//! and renders the headline table of `benchmarks/README.md`. It reads the snapshots the
//! `tracks` tests write (ADR 0012) and runs no game.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::cli::VerdictArgs;
use crate::tracks::{TRACKS, Tier, Track, TrackTargets};

/// A headline Track may not regress by more than this many guesses.
const HEADLINE_REGRESSION: f64 = 0.005;
/// A breadth Track may not regress by more than this many guesses.
const BREADTH_REGRESSION: f64 = 0.02;
/// Within this average mean delta, the failure rate decides.
const TIE: f64 = 0.001;
/// Deltas below this are float noise, not change.
const EPSILON: f64 = 1e-9;

pub const README_START: &str = "<!-- tracks:start -->";
pub const README_END: &str = "<!-- tracks:end -->";

#[derive(Debug, Clone, Copy, Deserialize)]
struct Stats {
    mean: f64,
    failure_rate: f64,
}

#[derive(Debug, Deserialize)]
struct Summary {
    uniform: Stats,
    prior: Stats,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ProfileSums {
    answers_sha256: String,
    guess_only_sha256: String,
}

#[derive(Debug, Deserialize)]
struct Body {
    track: String,
    corpus_sha256: String,
    profile: Option<ProfileSums>,
    summary: Summary,
}

/// One Track's committed snapshot: the body fields the verdict reads and the header digest.
#[derive(Debug)]
struct Snap {
    body: Body,
    digest: Option<String>,
}

/// The YAML header of a `.snap` file; the keys the verdict does not read are ignored.
#[derive(Debug, Deserialize)]
struct Header {
    info: Option<Info>,
}

#[derive(Debug, Deserialize)]
struct Info {
    solver_digest: Option<String>,
}

/// Splits an insta snapshot into its YAML header and body: the file opens with a `---` line and
/// the header ends at the next one.
fn split_snapshot(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((&rest[..end], &rest[end + "\n---\n".len()..]))
}

impl Snap {
    fn load(path: &Path) -> Result<Snap> {
        let text =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let (header, body) = split_snapshot(&text)
            .with_context(|| format!("{} has no snapshot header", path.display()))?;
        let header: Header = yaml_serde::from_str(header)
            .with_context(|| format!("{} has an unreadable header", path.display()))?;
        let body: Body = serde_json::from_str(body)
            .with_context(|| format!("{} has no Track body", path.display()))?;
        let digest = header.info.and_then(|i| i.solver_digest);
        Ok(Snap { body, digest })
    }

    /// The uniform statistics on a Profile Track, the prior-weighted ones on a Corpus Track.
    fn score(&self) -> Stats {
        match self.body.profile {
            Some(_) => self.body.summary.uniform,
            None => self.body.summary.prior,
        }
    }

    /// Whether the Corpus or Profile lists differ, so the two scores are not comparable.
    fn same_data(&self, other: &Snap) -> bool {
        self.body.corpus_sha256 == other.body.corpus_sha256
            && self.body.profile == other.body.profile
    }
}

fn load_dir(dir: &Path) -> Result<BTreeMap<String, Snap>> {
    let mut snaps = BTreeMap::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "snap") {
            let snap = Snap::load(&path)?;
            snaps.insert(snap.body.track.clone(), snap);
        }
    }
    Ok(snaps)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accept,
    Unchanged,
    Reject,
    Rebaseline,
    Stale,
    /// No judged Track exists on both sides, so there is nothing to compare.
    New,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Accept => "accept",
            Outcome::Unchanged => "unchanged",
            Outcome::Reject => "reject",
            Outcome::Rebaseline => "rebaseline",
            Outcome::Stale => "stale",
            Outcome::New => "new",
        }
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::Reject | Outcome::Stale => 1,
            _ => 0,
        }
    }
}

#[derive(Debug)]
struct Row {
    id: String,
    tier: &'static str,
    judged: bool,
    base: Option<f64>,
    head: Option<f64>,
    status: String,
}

impl Row {
    fn delta(&self) -> Option<f64> {
        Some(self.head? - self.base?)
    }
}

#[derive(Debug)]
pub struct Verdict {
    rows: Vec<Row>,
    notes: Vec<String>,
    pub outcome: Outcome,
}

impl Verdict {
    /// The markdown report: a table per Track, the notes behind the outcome, then the outcome.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "| Track | Tier | Judged | Base | Head | Delta | Status |\n|---|---|---|---|---|---|---|\n",
        );
        let cell = |v: Option<f64>| v.map_or("-".to_owned(), |v| format!("{v:.4}"));
        for row in &self.rows {
            let delta = row.delta().map_or("-".to_owned(), |d| format!("{d:+.4}"));
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} |",
                row.id,
                row.tier,
                if row.judged { "yes" } else { "no" },
                cell(row.base),
                cell(row.head),
                delta,
                row.status
            );
        }
        out.push('\n');
        for note in &self.notes {
            let _ = writeln!(out, "- {note}");
        }
        let _ = writeln!(out, "Outcome: {}", self.outcome.name());
        out
    }
}

struct Meta {
    tier: Option<Tier>,
    judged: bool,
}

fn meta(id: &str) -> Meta {
    match Track::find(id) {
        Some(t) => Meta {
            tier: Some(t.tier),
            judged: t.judged,
        },
        None => Meta {
            tier: None,
            judged: false,
        },
    }
}

/// Tracks in table order, then any unknown ids alphabetically.
fn ordered_ids(base: &BTreeMap<String, Snap>, head: &BTreeMap<String, Snap>) -> Vec<String> {
    let mut ids: Vec<String> = TRACKS
        .iter()
        .map(Track::to_string)
        .filter(|id| base.contains_key(id) || head.contains_key(id))
        .collect();
    for id in base.keys().chain(head.keys()) {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

fn compare(
    base: &BTreeMap<String, Snap>,
    head: &BTreeMap<String, Snap>,
    expected_digest: &str,
) -> Verdict {
    let mut rows = Vec::new();
    let mut notes = Vec::new();
    let mut stale_headline = false;
    let mut excluded = 0usize;
    // (is headline, mean delta, failure-rate delta) of every Track the rule judges.
    let mut judged: Vec<(bool, f64, f64, String)> = Vec::new();

    for id in ordered_ids(base, head) {
        let m = meta(&id);
        let (b, h) = (base.get(&id), head.get(&id));
        let stale = h.is_some_and(|h| h.digest.as_deref() != Some(expected_digest));
        let counts = m.judged && matches!(m.tier, Some(Tier::Headline | Tier::Breadth));
        if stale && m.tier == Some(Tier::Headline) {
            stale_headline = true;
            notes.push(format!(
                "headline Track {id} was not run on the current solver"
            ));
        }
        let mut status = match (b, h) {
            (None, Some(_)) => "new".to_owned(),
            (Some(_), None) => "missing".to_owned(),
            (Some(b), Some(h)) if !b.same_data(h) => {
                if counts {
                    excluded += 1;
                }
                "corpus changed".to_owned()
            }
            (Some(b), Some(h)) if counts => {
                let (bs, hs) = (b.score(), h.score());
                let delta = hs.mean - bs.mean;
                judged.push((
                    m.tier == Some(Tier::Headline),
                    delta,
                    hs.failure_rate - bs.failure_rate,
                    id.clone(),
                ));
                match delta {
                    d if d < -EPSILON => "better",
                    d if d > EPSILON => "worse",
                    _ => "same",
                }
                .to_owned()
            }
            (Some(_), Some(_)) => "not judged".to_owned(),
            (None, None) => unreachable!("ids come from the two sets"),
        };
        if stale {
            status.push_str(" (stale)");
        }
        rows.push(Row {
            id,
            tier: m.tier.map_or("unknown", Tier::name),
            judged: counts,
            base: b.map(|s| s.score().mean),
            head: h.map(|s| s.score().mean),
            status,
        });
    }

    let outcome = decide(&judged, excluded, stale_headline, &mut notes);
    Verdict {
        rows,
        notes,
        outcome,
    }
}

fn decide(
    judged: &[(bool, f64, f64, String)],
    excluded: usize,
    stale_headline: bool,
    notes: &mut Vec<String>,
) -> Outcome {
    if stale_headline {
        return Outcome::Stale;
    }
    if judged.is_empty() {
        return if excluded > 0 {
            notes.push("every judged Track changed Corpus: rebaseline".to_owned());
            Outcome::Rebaseline
        } else {
            Outcome::New
        };
    }
    let mut reject = false;
    for (headline, delta, _, id) in judged {
        let limit = if *headline {
            HEADLINE_REGRESSION
        } else {
            BREADTH_REGRESSION
        };
        if *delta > limit {
            reject = true;
            notes.push(format!("{id} is worse by {delta:.4} (limit {limit})"));
        }
    }
    if judged
        .iter()
        .all(|(_, d, f, _)| d.abs() < EPSILON && f.abs() < EPSILON)
    {
        return if reject {
            Outcome::Reject
        } else {
            Outcome::Unchanged
        };
    }
    // The average is over the headline Tracks, or over the breadth Tracks when none was judged.
    let tier_set: Vec<&(bool, f64, f64, String)> = {
        let headline: Vec<_> = judged.iter().filter(|j| j.0).collect();
        if headline.is_empty() {
            judged.iter().collect()
        } else {
            headline
        }
    };
    let n = tier_set.len() as f64;
    let mean = tier_set.iter().map(|j| j.1).sum::<f64>() / n;
    let failure = tier_set.iter().map(|j| j.2).sum::<f64>() / n;
    notes.push(format!(
        "average mean delta {mean:+.4}, average failure-rate delta {failure:+.4}"
    ));
    let better = if mean < -TIE {
        true
    } else if mean <= TIE {
        failure < -EPSILON
    } else {
        false
    };
    if !better {
        reject = true;
        notes.push(if mean.abs() <= TIE {
            "within the tie band and the failure rate did not fall".to_owned()
        } else {
            "the average mean delta is not negative".to_owned()
        });
    }
    if reject {
        Outcome::Reject
    } else {
        Outcome::Accept
    }
}

/// Judges `head` against `base`; `expected_digest` is the solver digest the headline snapshots
/// must carry. A missing `base` directory counts every Track as new.
pub fn verdict(base: &Path, head: &Path, expected_digest: &str) -> Result<Verdict> {
    if !head.is_dir() {
        bail!("{} is not a directory", head.display());
    }
    let head = load_dir(head)?;
    let base = if base.is_dir() {
        load_dir(base)?
    } else {
        BTreeMap::new()
    };
    Ok(compare(&base, &head, expected_digest))
}

/// The headline-Tier table of the head snapshots, judged and reported.
pub fn readme_table(head: &Path) -> Result<String> {
    let snaps = load_dir(head)?;
    let mut out = String::from(
        "| Track | Targets | Uniform mean | Prior-weighted mean | Failure rate (>6) | Corpus |\n\
         |---|---|---|---|---|---|\n",
    );
    for track in TRACKS.iter().filter(|t| t.tier == Tier::Headline) {
        let id = track.to_string();
        let Some(snap) = snaps.get(&id) else { continue };
        let targets = match track.targets {
            TrackTargets::Full => "full".to_owned(),
            TrackTargets::Sample { n, .. } => format!("{n} sampled"),
        };
        let s = &snap.body.summary;
        let _ = writeln!(
            out,
            "| {id} | {targets} | {:.4} | {:.4} | {:.2}% | `{}` |",
            s.uniform.mean,
            s.prior.mean,
            s.uniform.failure_rate * 100.0,
            snap.body
                .corpus_sha256
                .get(..12)
                .unwrap_or(&snap.body.corpus_sha256)
        );
    }
    Ok(out)
}

/// Replaces the text between the README markers with `table`.
pub fn splice_readme(readme: &str, table: &str) -> Result<String> {
    let start = readme
        .find(README_START)
        .with_context(|| format!("missing {README_START}"))?
        + README_START.len();
    let end = readme
        .find(README_END)
        .with_context(|| format!("missing {README_END}"))?;
    if end < start {
        bail!("{README_END} comes before {README_START}");
    }
    Ok(format!("{}\n{}{}", &readme[..start], table, &readme[end..]))
}

fn run(args: &VerdictArgs) -> Result<i32> {
    if let Some(snaps) = &args.readme {
        let table = readme_table(snaps)?;
        let current = fs::read_to_string(&args.file)
            .with_context(|| format!("reading {}", args.file.display()))?;
        let updated = splice_readme(&current, &table)?;
        if args.check {
            if updated != current {
                eprintln!(
                    "{} is stale; run `lex verdict --readme {}`",
                    args.file.display(),
                    snaps.display()
                );
                return Ok(1);
            }
        } else if updated != current {
            fs::write(&args.file, updated)?;
        }
        return Ok(0);
    }
    let (base, head) = match (&args.base, &args.head) {
        (Some(base), Some(head)) => (base, head),
        _ => bail!("verdict needs a base and a head directory"),
    };
    let verdict = verdict(base, head, lex_core::SOLVER_DIGEST)?;
    print!("{}", verdict.render());
    Ok(verdict.outcome.exit_code())
}

/// Runs the subcommand and returns its exit code: 0 accept, unchanged, rebaseline or new;
/// 1 reject, stale or a stale README; 2 bad input.
pub fn command(args: &VerdictArgs) -> i32 {
    run(args).unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        2
    })
}
