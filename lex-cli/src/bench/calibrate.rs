//! Calibration: run each Track as a child `lex simulate`, pick its targets, and record its cost.

use anyhow::{Context, bail};

use super::manifest::{BREADTH_SAMPLE, Calibration, Manifest, SMOKE_SAMPLE, Targets, Tier, Track};

/// The budget of a Track (ADR 0007): 10 minutes of CPU.
pub const CPU_CUTOFF_S: f64 = 600.0;

/// A single process using more than this many cores makes the machine loaded.
const BUSY_PROCESS_CORES: f64 = 0.5;
/// All other processes together using more than this many cores makes the machine loaded.
const BUSY_TOTAL_CORES: f64 = 1.0;

/// What one child run cost.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunResult {
    pub cpu_s: f64,
    pub wall_s: f64,
    /// The run was killed on reaching the CPU cutoff.
    pub timed_out: bool,
}

/// Runs `simulate` with the given arguments; the seam that lets tests skip real child processes.
pub trait Runner {
    fn run(&mut self, simulate_args: &[String], cpu_cutoff_s: f64) -> anyhow::Result<RunResult>;
}

/// One other process's CPU use over the sampling window, in cores.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessLoad {
    pub pid: u32,
    pub name: String,
    pub cores: f64,
}

/// Samples the CPU every other process uses over a window (5 s in the real probe).
pub trait LoadProbe {
    fn sample(&mut self) -> anyhow::Result<Vec<ProcessLoad>>;
}

/// Why a machine counts as loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Contention {
    pub top: ProcessLoad,
    pub total_cores: f64,
}

impl std::fmt::Display for Contention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the machine is loaded: {} (pid {}) uses {:.0}% of a core, and other processes use {:.0}% in all",
            self.top.name,
            self.top.pid,
            self.top.cores * 100.0,
            self.total_cores * 100.0
        )
    }
}

/// `Some` when any process uses over half a core or all of them together use over one core.
pub fn contention(loads: &[ProcessLoad]) -> Option<Contention> {
    let top = loads.iter().max_by(|a, b| a.cores.total_cmp(&b.cores))?;
    let total_cores: f64 = loads.iter().map(|p| p.cores).sum();
    (top.cores > BUSY_PROCESS_CORES || total_cores > BUSY_TOTAL_CORES).then(|| Contention {
        top: top.clone(),
        total_cores,
    })
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn over_budget(r: &RunResult, cutoff_s: f64) -> bool {
    r.timed_out || r.cpu_s > cutoff_s
}

/// Picks a Track's targets and measures it. Headline Tracks always run in full and smoke Tracks
/// always on 100 targets; a breadth Track runs in full under the cutoff and falls back to 500
/// sampled targets when that does not finish. A Track over budget keeps its Tier and targets:
/// ADR 0007 fixes Tiers by length, not machine speed.
pub fn calibrate_track(
    track: &Track,
    runner: &mut dyn Runner,
    cutoff_s: f64,
) -> anyhow::Result<(Targets, Calibration)> {
    let mut play = |targets| -> anyhow::Result<(Targets, RunResult)> {
        let result = runner
            .run(&track.simulate_args(targets), cutoff_s)
            .with_context(|| format!("running track {}", track.id))?;
        Ok((targets, result))
    };
    let (targets, result) = match track.tier {
        Tier::Headline => play(Targets::Full)?,
        Tier::Smoke => play(Targets::Sample(SMOKE_SAMPLE))?,
        Tier::Breadth => {
            let (targets, full) = play(Targets::Full)?;
            if over_budget(&full, cutoff_s) {
                play(Targets::Sample(BREADTH_SAMPLE))?
            } else {
                (targets, full)
            }
        }
    };
    Ok((
        targets,
        Calibration {
            cpu_s: round2(result.cpu_s),
            wall_s: round2(result.wall_s),
            over_budget: over_budget(&result, cutoff_s),
            contended: false,
        },
    ))
}

/// Everything `calibrate` needs from the outside world.
pub struct Environment<'a> {
    pub runner: &'a mut dyn Runner,
    pub probe: &'a mut dyn LoadProbe,
    pub commit: Option<String>,
    pub cpu_model: String,
    pub cutoff_s: f64,
    pub force: bool,
}

/// Calibrates the Tracks at `indices` one at a time, calling `save` after each so an interrupted
/// run keeps what it measured. Before each Track the machine must be idle; with `force` a loaded
/// machine is allowed and the Track records `contended: true`.
pub fn calibrate(
    manifest: &mut Manifest,
    indices: &[usize],
    env: &mut Environment,
    save: &mut dyn FnMut(&Manifest) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    manifest.commit = env.commit.clone();
    manifest.cpu_model = Some(env.cpu_model.clone());
    for &i in indices {
        let id = manifest.tracks[i].id.clone();
        if manifest.tracks[i].pending {
            bail!("track {id} is pending and cannot be calibrated");
        }
        let busy = contention(&env.probe.sample()?);
        if let Some(c) = &busy {
            if !env.force {
                bail!("refusing to calibrate {id}: {c} (--force overrides)");
            }
            eprintln!("warning: calibrating {id} anyway: {c}");
        }
        eprintln!("calibrating {id}");
        let (targets, mut calibration) =
            calibrate_track(&manifest.tracks[i], env.runner, env.cutoff_s)?;
        calibration.contended = busy.is_some();
        if calibration.over_budget {
            eprintln!(
                "warning: {id} is over budget ({:.0} s of CPU against {:.0} s)",
                calibration.cpu_s, env.cutoff_s
            );
        }
        let track = &mut manifest.tracks[i];
        track.targets = targets;
        track.calibration = Some(calibration);
        save(manifest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers each run from a script keyed by whether it is a full run or a sample.
    struct FakeRunner {
        full: RunResult,
        sample: RunResult,
        calls: Vec<Vec<String>>,
    }

    impl FakeRunner {
        fn new(full_cpu: f64, sample_cpu: f64) -> Self {
            let r = |cpu_s| RunResult {
                cpu_s,
                wall_s: cpu_s + 1.0,
                timed_out: false,
            };
            FakeRunner {
                full: r(full_cpu),
                sample: r(sample_cpu),
                calls: Vec::new(),
            }
        }

        /// A run that reaches the cutoff is killed there.
        fn kill_at_cutoff(r: &mut RunResult, cutoff: f64) {
            if r.cpu_s > cutoff {
                *r = RunResult {
                    cpu_s: cutoff,
                    wall_s: cutoff + 2.0,
                    timed_out: true,
                };
            }
        }
    }

    impl Runner for FakeRunner {
        fn run(&mut self, args: &[String], cutoff: f64) -> anyhow::Result<RunResult> {
            self.calls.push(args.to_vec());
            let mut r = if args.iter().any(|a| a == "-n") {
                self.sample
            } else {
                self.full
            };
            Self::kill_at_cutoff(&mut r, cutoff);
            Ok(r)
        }
    }

    struct FakeProbe(Vec<Vec<ProcessLoad>>);

    impl LoadProbe for FakeProbe {
        fn sample(&mut self) -> anyhow::Result<Vec<ProcessLoad>> {
            Ok(if self.0.len() > 1 {
                self.0.remove(0)
            } else {
                self.0[0].clone()
            })
        }
    }

    fn load(name: &str, cores: f64) -> ProcessLoad {
        ProcessLoad {
            pid: 1234,
            name: name.into(),
            cores,
        }
    }

    fn track(id: &str) -> Track {
        Manifest::generated()
            .tracks
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    const CUTOFF: f64 = 600.0;

    #[test]
    fn a_breadth_track_that_fits_the_cutoff_runs_full() {
        let mut runner = FakeRunner::new(599.0, 300.0);
        let (targets, cal) = calibrate_track(&track("german-8"), &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Full);
        assert_eq!((cal.cpu_s, cal.over_budget), (599.0, false));
        assert_eq!(runner.calls.len(), 1);
        assert_eq!(runner.calls[0], ["-l", "8", "--lang", "german"]);
    }

    #[test]
    fn a_breadth_track_over_the_cutoff_falls_back_to_500_sampled_targets() {
        let mut runner = FakeRunner::new(1500.0, 580.0);
        let (targets, cal) = calibrate_track(&track("german-8"), &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Sample(500));
        // The recorded cost is the sampled run's, not the abandoned full run's.
        assert_eq!(
            (cal.cpu_s, cal.wall_s, cal.over_budget),
            (580.0, 581.0, false)
        );
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(
            runner.calls[1],
            ["-l", "8", "--lang", "german", "-n", "500", "--seed", "0"]
        );
    }

    #[test]
    fn a_full_run_exactly_at_the_cutoff_still_fits() {
        let mut runner = FakeRunner::new(CUTOFF, 1.0);
        let (targets, cal) = calibrate_track(&track("german-8"), &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Full);
        assert!(!cal.over_budget);
    }

    #[test]
    fn a_breadth_track_whose_sample_is_also_too_slow_is_over_budget_and_stays_breadth() {
        let mut runner = FakeRunner::new(5000.0, 4000.0);
        let t = track("german-8");
        let (targets, cal) = calibrate_track(&t, &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Sample(500));
        assert!(cal.over_budget);
        assert_eq!(cal.cpu_s, CUTOFF);
        assert_eq!(t.tier, Tier::Breadth);
    }

    #[test]
    fn a_headline_track_always_runs_full_and_is_flagged_not_demoted() {
        let mut runner = FakeRunner::new(9000.0, 1.0);
        let (targets, cal) = calibrate_track(&track("german-5"), &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Full);
        assert!(cal.over_budget);
        assert_eq!(runner.calls.len(), 1);
    }

    #[test]
    fn a_smoke_track_always_runs_100_sampled_targets() {
        let mut runner = FakeRunner::new(1.0, 12.0);
        let (targets, cal) = calibrate_track(&track("french-6"), &mut runner, CUTOFF).unwrap();
        assert_eq!(targets, Targets::Sample(100));
        assert_eq!((cal.cpu_s, cal.over_budget), (12.0, false));
        assert_eq!(
            runner.calls,
            [["-l", "6", "--lang", "french", "-n", "100", "--seed", "0"]]
        );
    }

    #[test]
    fn a_smoke_track_over_the_cutoff_is_flagged() {
        let mut runner = FakeRunner::new(1.0, 700.0);
        let (_, cal) = calibrate_track(&track("french-6"), &mut runner, CUTOFF).unwrap();
        assert!(cal.over_budget);
    }

    #[test]
    fn contention_names_a_busy_process_or_a_busy_total() {
        assert_eq!(contention(&[]), None);
        assert_eq!(contention(&[load("a", 0.5), load("b", 0.4)]), None);
        let one = contention(&[load("a", 0.1), load("busy", 0.51)]).unwrap();
        assert_eq!(one.top.name, "busy");
        // Each under half a core, together over one: still loaded, and the top one is named.
        let many = contention(&[load("a", 0.4), load("b", 0.45), load("c", 0.3)]).unwrap();
        assert_eq!(many.top.name, "b");
        assert!((many.total_cores - 1.15).abs() < 1e-9);
    }

    fn run_calibrate(
        probe: Vec<Vec<ProcessLoad>>,
        force: bool,
        ids: &[&str],
    ) -> (anyhow::Result<()>, Manifest, usize) {
        let mut manifest = Manifest::generated();
        let indices: Vec<usize> = ids
            .iter()
            .map(|id| manifest.tracks.iter().position(|t| t.id == *id).unwrap())
            .collect();
        let mut runner = FakeRunner::new(10.0, 5.0);
        let mut probe = FakeProbe(probe);
        let mut saves = 0;
        let result = calibrate(
            &mut manifest,
            &indices,
            &mut Environment {
                runner: &mut runner,
                probe: &mut probe,
                commit: Some("abc123".into()),
                cpu_model: "Test CPU".into(),
                cutoff_s: CUTOFF,
                force,
            },
            &mut |_| {
                saves += 1;
                Ok(())
            },
        );
        (result, manifest, saves)
    }

    fn cal<'a>(m: &'a Manifest, id: &str) -> Option<&'a Calibration> {
        m.tracks
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .calibration
            .as_ref()
    }

    #[test]
    fn an_idle_machine_calibrates_and_saves_after_each_track() {
        let (result, m, saves) = run_calibrate(
            vec![vec![load("a", 0.1)]],
            false,
            &["english-5", "german-8"],
        );
        result.unwrap();
        assert_eq!(saves, 2);
        assert_eq!(m.commit.as_deref(), Some("abc123"));
        assert_eq!(m.cpu_model.as_deref(), Some("Test CPU"));
        assert!(!cal(&m, "english-5").unwrap().contended);
        assert!(cal(&m, "french-5").is_none());
    }

    #[test]
    fn a_loaded_machine_is_refused_naming_the_process_and_keeps_earlier_tracks() {
        let idle = vec![load("a", 0.1)];
        let busy = vec![load("cargo-build", 0.9)];
        let (result, m, saves) = run_calibrate(vec![idle, busy], false, &["english-5", "german-8"]);
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("cargo-build") && err.contains("german-8"),
            "{err}"
        );
        assert_eq!(saves, 1);
        assert!(cal(&m, "english-5").is_some());
        assert!(cal(&m, "german-8").is_none());
    }

    #[test]
    fn force_runs_on_a_loaded_machine_and_marks_only_that_track_contended() {
        let idle = vec![load("a", 0.1)];
        let busy = vec![load("cargo-build", 0.9)];
        let (result, m, _) = run_calibrate(vec![idle, busy], true, &["english-5", "german-8"]);
        result.unwrap();
        assert!(!cal(&m, "english-5").unwrap().contended);
        assert!(cal(&m, "german-8").unwrap().contended);
    }

    #[test]
    fn a_pending_track_cannot_be_calibrated() {
        let (result, _, saves) = run_calibrate(vec![vec![]], false, &["english-11"]);
        assert!(result.unwrap_err().to_string().contains("pending"));
        assert_eq!(saves, 0);
    }
}
