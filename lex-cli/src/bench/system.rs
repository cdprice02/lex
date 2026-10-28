//! The real machine behind calibration: child processes, process-table sampling, CPU model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

use super::calibrate::{LoadProbe, ProcessLoad, RunResult, Runner};

/// How long the load check watches the other processes before each Track.
const LOAD_WINDOW: Duration = Duration::from_secs(5);
/// How often a running child is polled for exit.
const POLL: Duration = Duration::from_millis(50);
/// How often a running child's CPU is compared with the cutoff.
const CUTOFF_CHECK: Duration = Duration::from_secs(2);

/// One row of the process table.
#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    /// CPU seconds used so far, user plus system.
    pub cpu_s: f64,
}

/// `[[dd-]hh:]mm:ss[.cc]` as `ps` prints cumulative CPU time, in seconds.
fn parse_ps_time(s: &str) -> Option<f64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, rest)) => (d.parse::<f64>().ok()?, rest),
        None => (0.0, s),
    };
    let mut total = 0.0;
    for part in rest.split(':') {
        total = total * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(days * 86400.0 + total)
}

/// A row of `ps -A -o pid=,ppid=,time=,comm=`; the command name is last because it may hold spaces.
fn parse_ps_line(line: &str) -> Option<Proc> {
    let mut it = line.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let ppid = it.next()?.parse().ok()?;
    let cpu_s = parse_ps_time(it.next()?)?;
    let name = it.collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(Proc {
        pid,
        ppid,
        name,
        cpu_s,
    })
}

/// The contents of `/proc/<pid>/stat`. The command name sits in parentheses and may itself hold
/// spaces and parentheses, so the fields after it are found from the last `)`.
fn parse_proc_stat(text: &str, ticks_per_s: f64) -> Option<Proc> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let pid = text[..open].trim().parse().ok()?;
    let fields: Vec<&str> = text[close + 1..].split_whitespace().collect();
    let ppid = fields.get(1)?.parse().ok()?;
    let utime: f64 = fields.get(11)?.parse().ok()?;
    let stime: f64 = fields.get(12)?.parse().ok()?;
    Some(Proc {
        pid,
        ppid,
        name: text[open + 1..close].to_string(),
        cpu_s: (utime + stime) / ticks_per_s,
    })
}

fn ticks_per_second() -> f64 {
    // SAFETY: sysconf only reads a system constant.
    let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if t > 0 { t as f64 } else { 100.0 }
}

fn linux_snapshot() -> anyhow::Result<Vec<Proc>> {
    let ticks = ticks_per_second();
    let mut procs = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let path = entry?.path();
        let is_pid = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.bytes().all(|b| b.is_ascii_digit()));
        if !is_pid {
            continue;
        }
        // A process can exit between listing and reading.
        if let Ok(text) = std::fs::read_to_string(path.join("stat")) {
            procs.extend(parse_proc_stat(&text, ticks));
        }
    }
    Ok(procs)
}

fn ps_snapshot() -> anyhow::Result<Vec<Proc>> {
    let out = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,time=,comm="])
        .output()
        .context("running ps")?;
    if !out.status.success() {
        bail!("ps failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_ps_line)
        .collect())
}

/// Every process with its cumulative CPU time. Linux reads `/proc`, whose clock ticks resolve a
/// short window; elsewhere `ps` reports centiseconds.
fn snapshot() -> anyhow::Result<Vec<Proc>> {
    if Path::new("/proc/self/stat").exists() {
        linux_snapshot()
    } else {
        ps_snapshot()
    }
}

/// The CPU each process used between two snapshots, in cores, leaving out `root` and everything
/// descended from it (this process, its children, and the `ps` it spawns to look).
fn loads_between(before: &[Proc], after: &[Proc], elapsed_s: f64, root: u32) -> Vec<ProcessLoad> {
    let parent: HashMap<u32, u32> = after.iter().map(|p| (p.pid, p.ppid)).collect();
    let ours = |mut pid: u32| {
        // The table is acyclic, but bound the walk anyway.
        for _ in 0..parent.len() + 1 {
            if pid == root {
                return true;
            }
            match parent.get(&pid) {
                Some(&up) if up != pid => pid = up,
                _ => return false,
            }
        }
        false
    };
    let started: HashMap<u32, f64> = before.iter().map(|p| (p.pid, p.cpu_s)).collect();
    after
        .iter()
        .filter(|p| !ours(p.pid))
        .map(|p| ProcessLoad {
            pid: p.pid,
            name: p.name.rsplit('/').next().unwrap_or(&p.name).to_string(),
            cores: (p.cpu_s - started.get(&p.pid).copied().unwrap_or(0.0)).max(0.0) / elapsed_s,
        })
        .collect()
}

pub struct SystemProbe;

impl LoadProbe for SystemProbe {
    fn sample(&mut self) -> anyhow::Result<Vec<ProcessLoad>> {
        let before = snapshot()?;
        let start = Instant::now();
        std::thread::sleep(LOAD_WINDOW);
        let after = snapshot()?;
        Ok(loads_between(
            &before,
            &after,
            start.elapsed().as_secs_f64(),
            std::process::id(),
        ))
    }
}

/// Runs `<exe> simulate <args> --data-dir <dir>` as a child, measuring its CPU from the rusage
/// the kernel reports at reap, and killing it once its CPU passes the cutoff.
pub struct ChildRunner {
    pub exe: PathBuf,
    pub data_dir: PathBuf,
}

impl Runner for ChildRunner {
    fn run(&mut self, args: &[String], cpu_cutoff_s: f64) -> anyhow::Result<RunResult> {
        let mut child = Command::new(&self.exe)
            .arg("simulate")
            .args(args)
            .arg("--data-dir")
            .arg(&self.data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning {}", self.exe.display()))?;
        let pid = child.id() as libc::pid_t;
        let start = Instant::now();
        let mut next_check = start + CUTOFF_CHECK;
        let mut timed_out = false;
        loop {
            let mut status = 0;
            // SAFETY: an all-zero rusage is a valid value, and wait4 fills it for our own child.
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            // SAFETY: `pid` is our unreaped child and both pointers outlive the call.
            let reaped = unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, &mut usage) };
            if reaped < 0 {
                return Err(std::io::Error::last_os_error()).context("waiting for the child");
            }
            if reaped == pid {
                let wall_s = start.elapsed().as_secs_f64();
                if !timed_out && !(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0) {
                    bail!(
                        "`simulate {}` failed (wait status {status})",
                        args.join(" ")
                    );
                }
                let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
                return Ok(RunResult {
                    cpu_s: secs(usage.ru_utime) + secs(usage.ru_stime),
                    wall_s,
                    timed_out,
                });
            }
            std::thread::sleep(POLL);
            if !timed_out && Instant::now() >= next_check {
                next_check += CUTOFF_CHECK;
                let used = snapshot()?
                    .into_iter()
                    .find(|p| p.pid == child.id())
                    .map(|p| p.cpu_s);
                if used.is_some_and(|cpu| cpu > cpu_cutoff_s) {
                    timed_out = true;
                    child.kill().context("killing the child at the cutoff")?;
                }
            }
        }
    }
}

fn cpu_model_from_cpuinfo(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, model)| model.trim().to_string())
        .filter(|m| !m.is_empty())
}

/// The CPU model: `sysctl` on macOS, `/proc/cpuinfo` on Linux, else "unknown".
pub fn cpu_model() -> String {
    let from_sysctl = || {
        let out = Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        let model = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!model.is_empty()).then_some(model)
    };
    let from_cpuinfo = || cpu_model_from_cpuinfo(&std::fs::read_to_string("/proc/cpuinfo").ok()?);
    let model = if cfg!(target_os = "macos") {
        from_sysctl().or_else(from_cpuinfo)
    } else {
        from_cpuinfo().or_else(from_sysctl)
    };
    model.unwrap_or_else(|| "unknown".to_string())
}

/// The current commit, with `-dirty` appended when the tree has uncommitted changes.
pub fn git_commit() -> Option<String> {
    let git = |args: &[&str]| {
        let out = Command::new("git").args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let sha = git(&["rev-parse", "HEAD"])?;
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    Some(if dirty { format!("{sha}-dirty") } else { sha })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, ppid: u32, name: &str, cpu_s: f64) -> Proc {
        Proc {
            pid,
            ppid,
            name: name.into(),
            cpu_s,
        }
    }

    #[test]
    fn ps_times_parse_in_every_format() {
        assert_eq!(parse_ps_time("0:00.12"), Some(0.12));
        assert_eq!(parse_ps_time("12:34.50"), Some(754.5));
        assert_eq!(parse_ps_time("01:02:03"), Some(3723.0));
        assert_eq!(parse_ps_time("2-00:00:10"), Some(172810.0));
        assert_eq!(parse_ps_time("abc"), None);
    }

    #[test]
    fn ps_lines_keep_a_command_name_with_spaces() {
        assert_eq!(
            parse_ps_line("  412     1   0:01.50 /Applications/Google Chrome.app/Chrome"),
            Some(proc(412, 1, "/Applications/Google Chrome.app/Chrome", 1.5))
        );
        assert_eq!(parse_ps_line("garbage"), None);
    }

    #[test]
    fn proc_stat_survives_parentheses_and_spaces_in_the_name() {
        // Fields after the name: state ppid pgrp session tty tpgid flags minflt cminflt majflt
        // cmajflt utime stime.
        let line = "321 (my (odd) cmd) S 77 1 1 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1";
        assert_eq!(
            parse_proc_stat(line, 100.0),
            Some(proc(321, 77, "my (odd) cmd", 3.0))
        );
    }

    #[test]
    fn load_is_cpu_delta_over_elapsed_and_ours_are_excluded() {
        let before = [
            proc(1, 0, "launchd", 10.0),
            proc(10, 1, "lex", 5.0),
            proc(11, 10, "lex", 5.0),
            proc(20, 1, "/usr/bin/busy", 100.0),
        ];
        let after = [
            proc(1, 0, "launchd", 10.0),
            proc(10, 1, "lex", 9.0),
            proc(11, 10, "lex", 9.0),
            proc(12, 11, "ps", 0.1),
            proc(20, 1, "/usr/bin/busy", 104.0),
            proc(30, 1, "newcomer", 1.0),
        ];
        let loads = loads_between(&before, &after, 5.0, 10);
        let by_name: HashMap<&str, f64> =
            loads.iter().map(|p| (p.name.as_str(), p.cores)).collect();
        assert_eq!(by_name.len(), 3, "{loads:?}");
        assert_eq!(by_name["busy"], 0.8);
        assert_eq!(by_name["newcomer"], 0.2);
        assert_eq!(by_name["launchd"], 0.0);
    }

    #[test]
    fn cpu_model_reads_the_first_model_name() {
        let info = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Xeon(R) CPU\nmodel name\t: other\n";
        assert_eq!(
            cpu_model_from_cpuinfo(info).as_deref(),
            Some("Intel(R) Xeon(R) CPU")
        );
        assert_eq!(cpu_model_from_cpuinfo("processor\t: 0\n"), None);
    }

    #[test]
    fn the_real_process_table_lists_this_process() {
        let me = std::process::id();
        let table = snapshot().unwrap();
        assert!(table.iter().any(|p| p.pid == me));
    }

    #[test]
    fn a_child_run_reports_its_cpu_and_failures() {
        let mut runner = ChildRunner {
            exe: PathBuf::from("/bin/sh"),
            data_dir: PathBuf::from("data"),
        };
        // `sh simulate ...` cannot run; the fake "exe" is only here to exercise the failure path.
        assert!(runner.run(&["-c".into()], 600.0).is_err());
    }
}
