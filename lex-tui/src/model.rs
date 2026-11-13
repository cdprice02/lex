//! The Elm-style core of Watch: one [`Model`], the [`Msg`]s that change it and the pure
//! [`update`]. Anything slow or fallible leaves as a [`Request`] for the worker; its answer comes
//! back as a [`Msg`], so `update` never touches a thread or a terminal.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::session::{ExplanationView, GameInfo, Played, SessionError};

/// How a feedback pattern is written on a tile when colour is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Symbols {
    /// `g` / `y` / `x`
    #[default]
    Letters,
    /// `2` / `1` / `0`
    Digits,
    /// Coloured square emoji
    Emoji,
}

impl Symbols {
    /// The scheme the `s` key moves to.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Letters => Self::Digits,
            Self::Digits => Self::Emoji,
            Self::Emoji => Self::Letters,
        }
    }

    /// The name shown in the status bar.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Letters => "letters",
            Self::Digits => "digits",
            Self::Emoji => "emoji",
        }
    }
}

/// What the UI starts with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The seed of the first game; the next game uses the seed after it.
    pub seed: u64,
    /// Whether tiles are coloured.
    pub color: bool,
    /// How tiles are written when colour is off.
    pub symbols: Symbols,
}

/// Where the target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Drawn from the Answer list by this seed.
    Seed(u64),
    /// Typed by the player.
    Typed,
}

/// Where the game stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for the target.
    Starting,
    /// Waiting for the solver's explanation of the position.
    Thinking,
    /// The solver's guess is on screen; `space` plays it.
    Ready,
    /// Waiting for the feedback of the guess just played.
    Playing,
    /// The last guess was the target.
    Solved,
    /// The solver has no guess: no Candidate remains.
    Stuck,
    /// The session failed; the error is on screen and `n` starts another game.
    Failed,
}

/// The `/` entry box.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// What has been typed.
    pub text: String,
    /// Why the last submission was refused.
    pub error: Option<SessionError>,
}

/// The settings the player toggles with `r`, `d`, `c` and `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Display {
    /// Whether the target is shown before the game is solved.
    pub reveal: bool,
    /// Whether the explanation shows bits, every Guess cost and the opening book.
    pub details: bool,
    /// Whether tiles are coloured.
    pub color: bool,
    /// How tiles are written when colour is off.
    pub symbols: Symbols,
}

/// Everything the screen shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// The game being watched.
    pub game: GameInfo,
    /// Where the target came from.
    pub origin: Origin,
    /// The seed `n` advances from: the latest seed drawn.
    pub seed: u64,
    /// The target, once drawn or checked.
    pub target: Option<String>,
    /// The guesses played so far.
    pub history: Vec<Played>,
    /// The solver's explanation of the current position.
    pub explanation: Option<ExplanationView>,
    /// Where the game stands.
    pub phase: Phase,
    /// The latest session failure, until the next game starts.
    pub error: Option<SessionError>,
    /// Counts games; an answer tagged with an older number is dropped.
    pub epoch: u64,
    /// What the player has toggled.
    pub display: Display,
    /// The `/` box, while open.
    pub entry: Option<Entry>,
    /// Whether the player asked to leave.
    pub quit: bool,
}

/// An input to [`update`].
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// A key press.
    Key(KeyEvent),
    /// The target a seed drew.
    Drawn {
        /// The seed asked for.
        seed: u64,
        /// The target, or why none could be drawn.
        result: Result<String, SessionError>,
    },
    /// The verdict on a typed target.
    Checked {
        /// What was typed.
        word: String,
        /// The target, or why it is refused.
        result: Result<String, SessionError>,
    },
    /// The feedback of a guess played.
    Feedback {
        /// The game it belongs to.
        epoch: u64,
        /// The guess and its feedback, or why it could not be graded.
        result: Result<Played, SessionError>,
    },
    /// The solver's explanation of a position.
    Explained {
        /// The game it belongs to.
        epoch: u64,
        /// The explanation (`None` when no guess is possible), or why there is none.
        result: Result<Option<ExplanationView>, SessionError>,
    },
}

/// Work `update` hands to the worker, which answers each with a [`Msg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Draw the target `seed` selects.
    Draw {
        /// The seed.
        seed: u64,
    },
    /// Check that `word` is on the Answer list.
    Check {
        /// The typed word.
        word: String,
    },
    /// Play `guess` against `target`.
    Play {
        /// The game it belongs to.
        epoch: u64,
        /// The guess.
        guess: String,
        /// The target.
        target: String,
    },
    /// Explain the position after `history`.
    Explain {
        /// The game it belongs to.
        epoch: u64,
        /// The guesses so far.
        history: Vec<Played>,
    },
}

impl Model {
    /// The model at the start of the first game, and the request that draws its target.
    #[must_use]
    pub fn new(game: GameInfo, options: Options) -> (Self, Vec<Request>) {
        let model = Self {
            game,
            origin: Origin::Seed(options.seed),
            seed: options.seed,
            target: None,
            history: Vec::new(),
            explanation: None,
            phase: Phase::Starting,
            error: None,
            epoch: 0,
            display: Display {
                reveal: false,
                details: false,
                color: options.color,
                symbols: options.symbols,
            },
            entry: None,
            quit: false,
        };
        (model, vec![Request::Draw { seed: options.seed }])
    }

    /// Whether the target is on screen: after the game is solved, or when `r` asked.
    #[must_use]
    pub fn target_shown(&self) -> bool {
        self.display.reveal || self.phase == Phase::Solved
    }

    /// Starts a game with the target decided: the board is cleared and the opening position is
    /// asked for.
    fn begin(&mut self, origin: Origin, target: String) -> Vec<Request> {
        self.epoch += 1;
        self.origin = origin;
        self.target = Some(target);
        self.history.clear();
        self.explanation = None;
        self.error = None;
        self.phase = Phase::Thinking;
        vec![Request::Explain {
            epoch: self.epoch,
            history: Vec::new(),
        }]
    }
}

/// Applies `msg` to `model` and returns the work it asks for.
pub fn update(model: &mut Model, msg: Msg) -> Vec<Request> {
    match msg {
        Msg::Key(key) => on_key(model, key),
        Msg::Drawn { seed, result } => {
            if model.phase != Phase::Starting || model.origin != Origin::Seed(seed) {
                return Vec::new();
            }
            match result {
                Ok(target) => model.begin(Origin::Seed(seed), target),
                Err(error) => fail(model, error),
            }
        }
        Msg::Checked { word, result } => on_checked(model, &word, result),
        Msg::Feedback { epoch, result } => {
            if epoch != model.epoch || model.phase != Phase::Playing {
                return Vec::new();
            }
            let played = match result {
                Ok(played) => played,
                Err(error) => return fail(model, error),
            };
            let solved = played.is_correct();
            model.history.push(played);
            model.explanation = None;
            if solved {
                model.phase = Phase::Solved;
                return Vec::new();
            }
            model.phase = Phase::Thinking;
            vec![Request::Explain {
                epoch,
                history: model.history.clone(),
            }]
        }
        Msg::Explained { epoch, result } => {
            if epoch != model.epoch || model.phase != Phase::Thinking {
                return Vec::new();
            }
            match result {
                Ok(view) => {
                    model.phase = if view.is_some() {
                        Phase::Ready
                    } else {
                        Phase::Stuck
                    };
                    model.explanation = view;
                    Vec::new()
                }
                Err(error) => fail(model, error),
            }
        }
    }
}

/// Records a session failure: the game cannot go on, but `n` can start another.
fn fail(model: &mut Model, error: SessionError) -> Vec<Request> {
    model.error = Some(error);
    model.phase = Phase::Failed;
    Vec::new()
}

/// Applies the verdict on a typed target, when the `/` box is still open for it.
fn on_checked(model: &mut Model, word: &str, result: Result<String, SessionError>) -> Vec<Request> {
    let Some(entry) = model.entry.as_mut() else {
        return Vec::new();
    };
    if entry.text.trim().to_lowercase() != word {
        return Vec::new();
    }
    match result {
        Ok(target) => {
            model.entry = None;
            model.begin(Origin::Typed, target)
        }
        Err(error) => {
            entry.error = Some(error);
            Vec::new()
        }
    }
}

/// Applies a key press.
fn on_key(model: &mut Model, key: KeyEvent) -> Vec<Request> {
    let code = key.code;
    if key.modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        model.quit = true;
        return Vec::new();
    }
    if model.entry.is_some() {
        return on_entry_key(model, code);
    }
    if code == KeyCode::Char(' ') {
        return play_guess(model);
    }
    let KeyCode::Char(c) = code else {
        return Vec::new();
    };
    match c {
        'q' => model.quit = true,
        'n' => return new_game(model),
        '/' => model.entry = Some(Entry::default()),
        'r' => model.display.reveal = !model.display.reveal,
        'd' => model.display.details = !model.display.details,
        'c' => model.display.color = !model.display.color,
        's' => model.display.symbols = model.display.symbols.next(),
        _ => {}
    }
    Vec::new()
}

/// Plays the solver's guess when one is on screen.
fn play_guess(model: &mut Model) -> Vec<Request> {
    if model.phase != Phase::Ready {
        return Vec::new();
    }
    let (Some(view), Some(target)) = (&model.explanation, &model.target) else {
        return Vec::new();
    };
    let request = Request::Play {
        epoch: model.epoch,
        guess: view.guess.clone(),
        target: target.clone(),
    };
    model.phase = Phase::Playing;
    vec![request]
}

/// Abandons the game for one drawn from the next seed.
fn new_game(model: &mut Model) -> Vec<Request> {
    let seed = model.seed.wrapping_add(1);
    model.seed = seed;
    model.epoch += 1;
    model.origin = Origin::Seed(seed);
    model.target = None;
    model.history.clear();
    model.explanation = None;
    model.error = None;
    model.phase = Phase::Starting;
    vec![Request::Draw { seed }]
}

/// Applies a key to the open `/` box.
fn on_entry_key(model: &mut Model, code: KeyCode) -> Vec<Request> {
    let Some(entry) = model.entry.as_mut() else {
        return Vec::new();
    };
    if code == KeyCode::Esc {
        model.entry = None;
    } else if code == KeyCode::Backspace {
        entry.text.pop();
        entry.error = None;
    } else if code == KeyCode::Enter {
        let word = entry.text.trim().to_lowercase();
        if !word.is_empty() {
            entry.error = None;
            return vec![Request::Check { word }];
        }
    } else if let KeyCode::Char(c) = code
        && c.is_alphabetic()
        && entry.text.chars().count() < MAX_TYPED
    {
        entry.text.push(c);
        entry.error = None;
    }
    Vec::new()
}

/// The longest word the `/` box takes: the supported word lengths top out below it.
const MAX_TYPED: usize = 32;

#[cfg(test)]
mod tests {
    use lex_core::Correctness;

    use super::*;
    use crate::fake::{BROKEN, Broken, Call, Fake};
    use crate::runtime::{drive, handle, press};
    use crate::session::{Row, Session as _};

    const OPTIONS: Options = Options {
        seed: 0,
        color: true,
        symbols: Symbols::Letters,
    };

    fn key(code: KeyCode) -> Msg {
        Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn view(guess: &str) -> ExplanationView {
        ExplanationView {
            guess: guess.to_owned(),
            technique: Some("Entropy".to_owned()),
            from_book: false,
            candidates: 10,
            rows: vec![Row {
                word: guess.to_owned(),
                bits: 1.0,
                remaining: 2.0,
                cost: None,
                in_candidates: true,
            }],
        }
    }

    /// A model with its opening position explained, `slate` against `light`.
    fn ready() -> Model {
        drive(&mut Fake, OPTIONS, [])
    }

    /// Feeds `msg` to `model` through the fake session until nothing is pending.
    fn step(model: &mut Model, msg: Msg) {
        let mut queue: std::collections::VecDeque<Request> = update(model, msg).into();
        while let Some(request) = queue.pop_front() {
            queue.extend(update(model, handle(&mut Fake, request)));
        }
    }

    #[test]
    fn opens_by_drawing_the_seeds_target_then_explaining_the_opening() {
        let (mut model, requests) = Model::new(Fake.game(), OPTIONS);
        assert_eq!(requests, [Request::Draw { seed: 0 }]);
        assert_eq!(model.phase, Phase::Starting);
        let requests = update(
            &mut model,
            Msg::Drawn {
                seed: 0,
                result: Ok("light".to_owned()),
            },
        );
        assert_eq!(
            requests,
            [Request::Explain {
                epoch: 1,
                history: Vec::new()
            }]
        );
        assert_eq!(model.phase, Phase::Thinking);
        update(
            &mut model,
            Msg::Explained {
                epoch: 1,
                result: Ok(Some(view("slate"))),
            },
        );
        assert_eq!(model.phase, Phase::Ready);
    }

    #[test]
    fn space_plays_the_guess_and_asks_for_the_next_position() {
        let mut model = ready();
        let requests = update(&mut model, press(' '));
        assert_eq!(
            requests,
            [Request::Play {
                epoch: 1,
                guess: "slate".to_owned(),
                target: "light".to_owned()
            }]
        );
        assert_eq!(model.phase, Phase::Playing);
        // A second press while the feedback is pending does nothing.
        assert!(update(&mut model, press(' ')).is_empty());
        let played = Played {
            word: "slate".to_owned(),
            feedback: vec![Correctness::Absent; 5],
        };
        let requests = update(
            &mut model,
            Msg::Feedback {
                epoch: 1,
                result: Ok(played.clone()),
            },
        );
        assert_eq!(
            requests,
            [Request::Explain {
                epoch: 1,
                history: vec![played]
            }]
        );
        assert_eq!(model.phase, Phase::Thinking);
    }

    #[test]
    fn space_does_nothing_while_the_solver_thinks() {
        let (mut model, _) = Model::new(Fake.game(), OPTIONS);
        assert!(update(&mut model, press(' ')).is_empty());
        assert_eq!(model.phase, Phase::Starting);
    }

    #[test]
    fn a_correct_guess_solves_the_game() {
        let mut model = ready();
        for _ in 0..4 {
            step(&mut model, press(' '));
        }
        assert_eq!(model.phase, Phase::Solved);
        assert_eq!(model.history.len(), 4);
        assert!(model.target_shown());
        assert!(update(&mut model, press(' ')).is_empty());
    }

    #[test]
    fn a_game_runs_past_six_guesses() {
        let mut model = ready();
        model.target = Some("zzzzz".to_owned());
        for _ in 0..8 {
            step(&mut model, press(' '));
        }
        assert_eq!(model.history.len(), 8);
        assert_eq!(model.phase, Phase::Ready);
    }

    #[test]
    fn no_guess_leaves_the_game_stuck() {
        let mut model = ready();
        update(&mut model, press(' '));
        update(
            &mut model,
            Msg::Feedback {
                epoch: 1,
                result: Ok(Played {
                    word: "slate".to_owned(),
                    feedback: vec![Correctness::Absent; 5],
                }),
            },
        );
        update(
            &mut model,
            Msg::Explained {
                epoch: 1,
                result: Ok(None),
            },
        );
        assert_eq!(model.phase, Phase::Stuck);
    }

    #[test]
    fn n_draws_the_next_seed_and_drops_the_old_games_answers() {
        let mut model = ready();
        let requests = update(&mut model, press('n'));
        assert_eq!(requests, [Request::Draw { seed: 1 }]);
        assert_eq!(model.origin, Origin::Seed(1));
        assert_eq!((model.phase, model.history.len()), (Phase::Starting, 0));
        // The old game's explanation arrives late and is ignored.
        update(
            &mut model,
            Msg::Explained {
                epoch: 1,
                result: Ok(Some(view("slate"))),
            },
        );
        assert_eq!(model.phase, Phase::Starting);
        // So does a draw for a seed that was already replaced.
        assert!(
            update(
                &mut model,
                Msg::Drawn {
                    seed: 0,
                    result: Ok("light".to_owned())
                }
            )
            .is_empty()
        );
        step(&mut model, press('n'));
        assert_eq!(model.target.as_deref(), Some("mount"));
        assert_eq!(model.origin, Origin::Seed(2));
    }

    #[test]
    fn slash_types_a_word_and_a_valid_one_starts_a_typed_game() {
        let mut model = ready();
        update(&mut model, press('/'));
        assert_eq!(model.entry, Some(Entry::default()));
        for c in "Crane".chars() {
            update(&mut model, press(c));
        }
        let requests = update(&mut model, key(KeyCode::Enter));
        assert_eq!(
            requests,
            [Request::Check {
                word: "crane".to_owned()
            }]
        );
        for msg in requests.into_iter().map(|r| handle(&mut Fake, r)) {
            update(&mut model, msg);
        }
        assert_eq!(model.entry, None);
        assert_eq!(model.origin, Origin::Typed);
        assert_eq!(model.target.as_deref(), Some("crane"));
        assert_eq!((model.phase, model.history.len()), (Phase::Thinking, 0));
    }

    #[test]
    fn slash_keeps_the_box_open_on_an_invalid_word() {
        let mut model = ready();
        update(&mut model, press('/'));
        for c in "zebra".chars() {
            update(&mut model, press(c));
        }
        let before = model.clone();
        step(&mut model, key(KeyCode::Enter));
        let entry = model.entry.as_ref().unwrap();
        assert_eq!(entry.text, "zebra");
        assert_eq!(
            entry.error.as_ref().map(SessionError::message),
            Some("zebra is not in the wordle answer list")
        );
        assert_eq!(model.target, before.target);
        assert_eq!(model.history, before.history);
        // Typing clears the message; backspace edits; letters only.
        update(&mut model, key(KeyCode::Backspace));
        update(&mut model, press('1'));
        let entry = model.entry.as_ref().unwrap();
        assert_eq!((entry.text.as_str(), entry.error.as_ref()), ("zebr", None));
        update(&mut model, key(KeyCode::Esc));
        assert_eq!(model.entry, None);
    }

    #[test]
    fn keys_type_into_the_box_instead_of_acting() {
        let mut model = ready();
        update(&mut model, press('/'));
        for c in "qndrcs".chars() {
            update(&mut model, press(c));
        }
        assert!(!model.quit);
        assert_eq!(
            model.entry.as_ref().map(|e| e.text.as_str()),
            Some("qndrcs")
        );
        assert!(!model.display.details && !model.display.reveal && model.display.color);
    }

    #[test]
    fn r_d_c_s_toggle_their_settings() {
        let mut model = ready();
        update(&mut model, press('r'));
        assert!(model.display.reveal && model.target_shown());
        update(&mut model, press('r'));
        assert!(!model.display.reveal);
        update(&mut model, press('d'));
        assert!(model.display.details);
        update(&mut model, press('c'));
        assert!(!model.display.color);
        let schemes: Vec<Symbols> = (0..3)
            .map(|_| {
                update(&mut model, press('s'));
                model.display.symbols
            })
            .collect();
        assert_eq!(schemes, [Symbols::Digits, Symbols::Emoji, Symbols::Letters]);
    }

    fn error_message(model: &Model) -> Option<&str> {
        model.error.as_ref().map(SessionError::message)
    }

    #[test]
    fn a_failed_draw_stops_the_game_and_n_tries_again() {
        let mut model = drive(&mut Broken(Call::Draw), OPTIONS, []);
        assert_eq!(model.phase, Phase::Failed);
        assert_eq!(error_message(&model), Some(BROKEN));
        assert_eq!(model.target, None);
        let requests = update(&mut model, press('n'));
        assert_eq!(requests, [Request::Draw { seed: 1 }]);
        assert_eq!((model.phase, model.error.as_ref()), (Phase::Starting, None));
    }

    #[test]
    fn a_failed_check_keeps_the_box_open_with_the_error() {
        let mut model = ready();
        update(&mut model, press('/'));
        for c in "crane".chars() {
            update(&mut model, press(c));
        }
        let requests = update(&mut model, key(KeyCode::Enter));
        for request in requests {
            let msg = handle(&mut Broken(Call::CheckTarget), request);
            update(&mut model, msg);
        }
        let entry = model.entry.as_ref().unwrap();
        assert_eq!(
            entry.error.as_ref().map(SessionError::message),
            Some(BROKEN)
        );
        assert_eq!(model.origin, Origin::Seed(0));
        assert_eq!(model.phase, Phase::Ready);
    }

    #[test]
    fn a_failed_play_stops_the_game_with_the_guess_unplayed() {
        let mut model = ready();
        let requests = update(&mut model, press(' '));
        for request in requests {
            let msg = handle(&mut Broken(Call::Play), request);
            assert!(update(&mut model, msg).is_empty());
        }
        assert_eq!(model.phase, Phase::Failed);
        assert_eq!(error_message(&model), Some(BROKEN));
        assert!(model.history.is_empty());
    }

    #[test]
    fn a_failed_explain_stops_the_game() {
        let mut model = ready();
        model.phase = Phase::Thinking;
        model.explanation = None;
        let msg = handle(
            &mut Broken(Call::Explain),
            Request::Explain {
                epoch: model.epoch,
                history: Vec::new(),
            },
        );
        assert!(update(&mut model, msg).is_empty());
        assert_eq!(model.phase, Phase::Failed);
        assert_eq!(error_message(&model), Some(BROKEN));
    }

    #[test]
    fn a_failure_from_an_abandoned_game_is_ignored() {
        let mut model = ready();
        update(&mut model, press('n'));
        let stale = Msg::Explained {
            epoch: 1,
            result: Err(SessionError::new("late")),
        };
        assert!(update(&mut model, stale).is_empty());
        assert_eq!((model.phase, model.error), (Phase::Starting, None));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut model = ready();
        update(&mut model, press('q'));
        assert!(model.quit);
        let mut model = ready();
        update(
            &mut model,
            Msg::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        );
        assert!(model.quit);
    }
}
