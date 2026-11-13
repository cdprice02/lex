//! The thin shell around the model: the worker that answers requests, the terminal loop, and a
//! headless driver that plays a game with no terminal and no thread.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::model::{Model, Msg, Options, Request, update};
use crate::session::{Played, Session};
use crate::view;

/// How long the loop waits for a key before it drains the worker and draws again.
const TICK: Duration = Duration::from_millis(50);

/// The key press of the character `c`, as a [`Msg`].
#[must_use]
pub const fn press(c: char) -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// The enter key, as a [`Msg`].
#[must_use]
pub const fn enter() -> Msg {
    Msg::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

/// Answers one request from the worker's session.
pub fn handle(session: &mut dyn Session, request: Request) -> Msg {
    match request {
        Request::Draw { seed } => Msg::Drawn {
            seed,
            target: session.draw(seed),
        },
        Request::Check { word } => {
            let result = session.check_target(&word);
            Msg::Checked { word, result }
        }
        Request::Play {
            epoch,
            guess,
            target,
        } => {
            let feedback = session.play(&guess, &target);
            Msg::Feedback {
                epoch,
                played: Played {
                    word: guess,
                    feedback,
                },
            }
        }
        Request::Explain { epoch, history } => Msg::Explained {
            epoch,
            view: session.explain(&history),
        },
    }
}

/// Plays Watch with no terminal and no thread: opens a game, then feeds `inputs` in order,
/// answering every request on the spot. Returns the model where the inputs leave it.
pub fn drive(
    session: &mut dyn Session,
    options: Options,
    inputs: impl IntoIterator<Item = Msg>,
) -> Model {
    let (mut model, requests) = Model::new(session.game(), options);
    settle(session, &mut model, requests);
    for input in inputs {
        let requests = update(&mut model, input);
        settle(session, &mut model, requests);
    }
    model
}

/// Answers `requests`, and whatever those answers ask for, until none is left.
fn settle(session: &mut dyn Session, model: &mut Model, requests: Vec<Request>) {
    let mut queue: VecDeque<Request> = requests.into();
    while let Some(request) = queue.pop_front() {
        let msg = handle(session, request);
        queue.extend(update(model, msg));
    }
}

/// Runs Watch on the terminal until the player quits.
///
/// The solver runs on its own thread, so the screen keeps drawing and `q` keeps working while it
/// thinks. Quitting does not wait for it.
///
/// # Errors
///
/// If the terminal cannot be set up, drawn to or read.
pub fn run(session: Box<dyn Session + Send>, options: Options) -> io::Result<()> {
    let (model, requests) = Model::new(session.game(), options);
    let (to_worker, requests_in) = mpsc::channel::<Request>();
    let (answers_out, from_worker) = mpsc::channel::<Msg>();
    std::thread::spawn(move || {
        let mut session = session;
        for request in requests_in {
            if answers_out.send(handle(&mut *session, request)).is_err() {
                break;
            }
        }
    });
    for request in requests {
        send(&to_worker, request)?;
    }
    let mut terminal = ratatui::try_init()?;
    let result = event_loop(&mut terminal, model, &to_worker, &from_worker);
    ratatui::restore();
    result
}

/// Queues `request` for the worker.
fn send(to_worker: &mpsc::Sender<Request>, request: Request) -> io::Result<()> {
    to_worker
        .send(request)
        .map_err(|_| io::Error::other("the solver thread stopped"))
}

/// Draws, reads keys and drains the worker's answers until `model` asks to quit.
fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    mut model: Model,
    to_worker: &mpsc::Sender<Request>,
    from_worker: &mpsc::Receiver<Msg>,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| view::draw(frame, &model))?;
        let mut msgs = Vec::new();
        if event::poll(TICK)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            msgs.push(Msg::Key(key));
        }
        msgs.extend(from_worker.try_iter());
        for msg in msgs {
            for request in update(&mut model, msg) {
                send(to_worker, request)?;
            }
        }
        if model.quit {
            return Ok(());
        }
    }
}
