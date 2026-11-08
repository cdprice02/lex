//! Progress for the long phases of a run: building an Opening book and playing games.
//!
//! One channel with three modes ([`Mode`]), chosen once per process by [`init`]. Library callers
//! never initialise it and pass no [`Observer`], so they draw nothing. The commands ask
//! [`observer`] for the reporter of the active mode and hand it to `run`, `ensure` and `guesser`.

use std::cell::{Cell, RefCell};
use std::io::IsTerminal as _;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;

/// The log target of the periodic progress lines.
pub const TARGET: &str = "lex::progress";

/// The `env_logger` filter used when `RUST_LOG` is unset: errors and the progress lines.
pub const DEFAULT_FILTER: &str = "error,lex::progress=info";

/// How often [`Mode::Lines`] reports.
const LINE_INTERVAL: Duration = Duration::from_secs(5);

/// How progress is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Bars and a spinner on stderr, with log lines printed above them.
    Bar,
    /// A log line every few seconds under [`TARGET`].
    Lines,
    /// Nothing (`--no-progress`, or no mode chosen).
    Off,
}

/// The mode for a run: `--no-progress` wins, then a terminal on stderr gets bars, else lines.
#[must_use]
pub const fn mode(stderr_is_terminal: bool, no_progress: bool) -> Mode {
    if no_progress {
        Mode::Off
    } else if stderr_is_terminal {
        Mode::Bar
    } else {
        Mode::Lines
    }
}

/// The process-wide state set by [`init`].
struct State {
    /// The chosen mode.
    mode: Mode,
    /// The bars' host, present in [`Mode::Bar`] only.
    multi: Option<MultiProgress>,
}

/// Set once by [`init`]; unset means [`Mode::Off`].
static STATE: OnceLock<State> = OnceLock::new();

/// The active mode, [`Mode::Off`] before [`init`].
#[must_use]
pub fn current() -> Mode {
    STATE.get().map_or(Mode::Off, |state| state.mode)
}

/// Picks the mode from the environment and installs the logger.
///
/// In [`Mode::Bar`] the logger writes through the bars' host so a log line never tears a bar.
/// `RUST_LOG`, when set, replaces [`DEFAULT_FILTER`].
pub fn init(no_progress: bool) {
    let mode = mode(std::io::stderr().is_terminal(), no_progress);
    let logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(DEFAULT_FILTER))
            .build();
    let level = logger.filter();
    let multi = (mode == Mode::Bar).then(MultiProgress::new);
    // A second call leaves the first logger in place, which is the behaviour wanted.
    let installed = match &multi {
        Some(multi) => LogWrapper::new(multi.clone(), logger).try_init().is_ok(),
        None => log::set_boxed_logger(Box::new(logger)).is_ok(),
    };
    if installed {
        log::set_max_level(level);
    }
    let _ = STATE.set(State { mode, multi });
}

/// What a long phase reports as it runs. Every method does nothing by default; methods take
/// `&self` so one observer can be shared down the call chain.
pub trait Observer {
    /// A book is about to be built; `message` says for what.
    fn book_started(&self, _message: &str) {}
    /// One more position was booked; `booked` is the count so far.
    fn book_progress(&self, _booked: usize) {}
    /// The book was built and stored; `line` is its result line.
    fn book_finished(&self, _line: &str) {}
    /// `total` games are about to be played.
    fn games_started(&self, _total: usize) {}
    /// One game finished.
    fn game_played(&self) {}
}

/// The observer of the active mode, or `None` when progress is off.
#[must_use]
pub fn observer() -> Option<Box<dyn Observer>> {
    let state = STATE.get()?;
    match state.mode {
        Mode::Bar => state.multi.clone().map(|multi| {
            let bars: Box<dyn Observer> = Box::new(BarObserver::new(multi));
            bars
        }),
        Mode::Lines => Some(Box::new(LineObserver::new())),
        Mode::Off => None,
    }
}

/// Draws a spinner for the book and a bar for the games.
struct BarObserver {
    /// The host the bars are added to.
    multi: MultiProgress,
    /// The live spinner or bar, one phase at a time. Owned by this observer alone, behind a
    /// `RefCell` only because [`Observer`] methods take `&self`; no borrow outlives a method.
    bar: RefCell<Option<ProgressBar>>,
}

impl BarObserver {
    /// An observer drawing on `multi`.
    const fn new(multi: MultiProgress) -> Self {
        Self {
            multi,
            bar: RefCell::new(None),
        }
    }

    /// Replaces the live bar with `next`, clearing the old one.
    fn swap(&self, next: ProgressBar) -> ProgressBar {
        let next = self.multi.add(next);
        if let Some(old) = self.bar.borrow_mut().replace(next.clone()) {
            old.finish_and_clear();
        }
        next
    }
}

#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "indicatif templates use braces for their own placeholders"
)]
impl Observer for BarObserver {
    fn book_started(&self, message: &str) {
        // Printing fails only when stderr is gone.
        let _ = self.multi.println(message);
        let spinner = self.swap(ProgressBar::new_spinner());
        spinner.set_style(
            ProgressStyle::with_template("{spinner} booked {pos} positions ({elapsed})")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        spinner.enable_steady_tick(Duration::from_millis(100));
    }

    fn book_progress(&self, booked: usize) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.set_position(u64::try_from(booked).unwrap_or(u64::MAX));
        }
    }

    fn book_finished(&self, line: &str) {
        if let Some(bar) = self.bar.borrow_mut().take() {
            bar.set_style(
                ProgressStyle::with_template("{msg}")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            bar.finish_with_message(line.to_owned());
        }
    }

    fn games_started(&self, total: usize) {
        let bar = self.swap(ProgressBar::new(u64::try_from(total).unwrap_or(u64::MAX)));
        bar.set_style(
            ProgressStyle::with_template(
                "{bar:30} {pos}/{len} games {elapsed} {per_sec} eta {eta}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
        );
    }

    fn game_played(&self) {
        if let Some(bar) = self.bar.borrow().as_ref() {
            bar.inc(1);
        }
    }
}

impl Drop for BarObserver {
    fn drop(&mut self) {
        if let Some(bar) = self.bar.borrow_mut().take() {
            bar.finish();
        }
    }
}

/// Where the current phase stands: the one piece of state a [`LineObserver`] mutates.
#[derive(Debug, Clone, Copy)]
struct Phase {
    /// When the phase began.
    started: Instant,
    /// When a line was last logged.
    last: Instant,
    /// Games finished in the phase.
    played: usize,
    /// Games in the phase.
    total: usize,
}

impl Phase {
    /// A phase beginning now with no games.
    fn begin() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            played: 0,
            total: 0,
        }
    }

    /// Whether a line is due, resetting the timer when it is.
    fn due(&mut self) -> bool {
        let due = self.last.elapsed() >= LINE_INTERVAL;
        if due {
            self.last = Instant::now();
        }
        due
    }
}

/// Logs a line every [`LINE_INTERVAL`] under [`TARGET`].
struct LineObserver {
    /// The current phase. Owned by this observer alone, behind a `Cell` only because
    /// [`Observer`] methods take `&self`; it is read and written within one method at a time.
    phase: Cell<Phase>,
}

impl LineObserver {
    /// An observer whose clock starts now.
    fn new() -> Self {
        Self {
            phase: Cell::new(Phase::begin()),
        }
    }

    /// Applies `f` to the phase and stores the result, returning what `f` returns.
    fn update<T>(&self, f: impl FnOnce(&mut Phase) -> T) -> T {
        let mut phase = self.phase.get();
        let out = f(&mut phase);
        self.phase.set(phase);
        out
    }
}

impl Observer for LineObserver {
    fn book_started(&self, message: &str) {
        self.phase.set(Phase::begin());
        log::info!(target: TARGET, "{message}");
    }

    fn book_progress(&self, booked: usize) {
        let (due, started) = self.update(|phase| (phase.due(), phase.started));
        if due {
            log::info!(
                target: TARGET,
                "booked {booked} positions ({:?} elapsed)",
                started.elapsed()
            );
        }
    }

    fn book_finished(&self, line: &str) {
        log::info!(target: TARGET, "{line}");
    }

    fn games_started(&self, total: usize) {
        self.phase.set(Phase {
            total,
            ..Phase::begin()
        });
    }

    fn game_played(&self) {
        let (due, phase) = self.update(|phase| {
            phase.played += 1;
            (phase.due(), *phase)
        });
        if due {
            log::info!(
                target: TARGET,
                "played {}/{} games ({:?} elapsed)",
                phase.played,
                phase.total,
                phase.started.elapsed()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_is_a_function_of_the_terminal_and_the_flag() {
        assert_eq!(mode(true, false), Mode::Bar);
        assert_eq!(mode(false, false), Mode::Lines);
        assert_eq!(mode(true, true), Mode::Off);
        assert_eq!(mode(false, true), Mode::Off);
    }

    #[test]
    fn progress_is_off_until_initialised() {
        assert_eq!(current(), Mode::Off);
        assert!(observer().is_none());
    }
}
