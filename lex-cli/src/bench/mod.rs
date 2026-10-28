//! `lex bench`: the Suite manifest and the commands that read or recalibrate it.

mod calibrate;
mod manifest;
mod system;

use anyhow::{Context, bail};

use crate::cli::BenchArgs;
use manifest::Manifest;
pub use manifest::Tier;

/// The indices of the Tracks that `--track` and `--tier` select; all of them when neither is given.
fn select(
    manifest: &Manifest,
    tracks: &[String],
    tier: Option<Tier>,
) -> anyhow::Result<Vec<usize>> {
    for id in tracks {
        if !manifest.tracks.iter().any(|t| &t.id == id) {
            bail!("no track {id} in the manifest");
        }
    }
    Ok(manifest
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| tracks.is_empty() || tracks.contains(&t.id))
        .filter(|(_, t)| tier.is_none_or(|tier| t.tier == tier))
        .map(|(i, _)| i)
        .collect())
}

/// One tab-separated line per Track, for people and for `oracle.sh`: id, Tier, `reported` as
/// yes or no, the arguments that select the Track, and its full `simulate` command.
fn dry_run_line(track: &manifest::Track) -> String {
    format!(
        "{}\t{}\t{}\t{}\tlex simulate {}",
        track.id,
        track.tier.name(),
        if track.reported { "yes" } else { "no" },
        track.track_args().join(" "),
        track.simulate_args(track.targets).join(" "),
    )
}

pub fn bench(args: &BenchArgs) -> anyhow::Result<()> {
    if !args.dry_run && !args.calibrate {
        bail!("pass --dry-run or --calibrate; the suite run is not built yet");
    }
    if args.force && !args.calibrate {
        bail!("--force only applies to --calibrate");
    }

    // A calibration may start a manifest from the generated matrix; nothing else may.
    let mut manifest = if args.calibrate && !args.manifest.exists() {
        Manifest::generated()
    } else {
        Manifest::load(&args.manifest)?
    };
    let drift = manifest.drift();
    if !drift.is_empty() {
        bail!(
            "{} drifts from the generated matrix:\n  {}",
            args.manifest.display(),
            drift.join("\n  ")
        );
    }
    let selected = select(&manifest, &args.track, args.tier)?;

    if args.dry_run {
        for &i in &selected {
            if !manifest.tracks[i].pending {
                println!("{}", dry_run_line(&manifest.tracks[i]));
            }
        }
        let pending = selected
            .iter()
            .filter(|&&i| manifest.tracks[i].pending)
            .count();
        eprintln!("{} tracks, {pending} pending", selected.len() - pending);
        return Ok(());
    }

    let indices: Vec<usize> = selected
        .into_iter()
        .filter(|&i| !manifest.tracks[i].pending)
        .collect();
    let mut runner = system::ChildRunner {
        exe: std::env::current_exe().context("locating the lex binary")?,
        data_dir: args.data_dir.clone(),
    };
    let mut probe = system::SystemProbe;
    let path = args.manifest.clone();
    calibrate::calibrate(
        &mut manifest,
        &indices,
        &mut calibrate::Environment {
            runner: &mut runner,
            probe: &mut probe,
            commit: system::git_commit(),
            cpu_model: system::cpu_model(),
            cutoff_s: calibrate::CPU_CUTOFF_S,
            force: args.force,
        },
        &mut |m| m.save(&path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_intersects_track_and_tier() {
        let m = Manifest::generated();
        let ids = |sel: Vec<usize>| -> Vec<String> {
            sel.into_iter().map(|i| m.tracks[i].id.clone()).collect()
        };
        assert_eq!(select(&m, &[], None).unwrap().len(), m.tracks.len());
        assert_eq!(
            ids(select(&m, &["wordle".into(), "french-6".into()], None).unwrap()),
            ["wordle", "french-6"]
        );
        assert_eq!(
            ids(select(
                &m,
                &["wordle".into(), "french-3".into()],
                Some(Tier::Breadth)
            )
            .unwrap()),
            ["french-3"]
        );
        assert!(select(&m, &["nope".into()], None).is_err());
    }

    #[test]
    fn a_dry_run_line_carries_the_oracle_fields_and_the_command() {
        let m = Manifest::generated();
        let t = m.tracks.iter().find(|t| t.id == "wordle-hard").unwrap();
        assert_eq!(
            dry_run_line(t),
            "wordle-hard\theadline\tyes\t--profile wordle --hard\tlex simulate --profile wordle --hard"
        );
        let t = m.tracks.iter().find(|t| t.id == "german-8").unwrap();
        assert_eq!(
            dry_run_line(t),
            "german-8\tbreadth\tno\t-l 8 --lang german\tlex simulate -l 8 --lang german -n 500 --seed 0"
        );
    }
}
