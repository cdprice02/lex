//! Drawing a [`Model`]: tiles on the left, the explanation on the right, a status bar below.
//! Designed for 80x24; a smaller terminal gets a notice instead.

use lex_core::Correctness;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use crate::model::{Entry, Model, Origin, Phase, Symbols};
use crate::session::{ExplanationView, Played};

/// The columns the layout is designed for.
pub const MIN_WIDTH: u16 = 80;
/// The rows the layout is designed for.
pub const MIN_HEIGHT: u16 = 24;

/// How many of the guesses the solver weighed the panel lists.
const SHOWN: usize = 5;

/// The least width of the board, whatever the word length.
const BOARD_MIN_WIDTH: u16 = 30;

/// Draws `model` on `frame`.
pub fn draw(frame: &mut Frame, model: &Model) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        draw_too_small(frame, area);
        return;
    }
    let [main, status] = Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas(area);
    let board_width = u16::try_from(model.game.word_length)
        .unwrap_or(u16::MAX)
        .saturating_mul(4)
        .saturating_add(4)
        .max(BOARD_MIN_WIDTH);
    let [board, explanation] =
        Layout::horizontal([Constraint::Length(board_width), Constraint::Min(0)]).areas(main);
    draw_board(frame, board, model);
    draw_explanation(frame, explanation, model);
    draw_status(frame, status, model);
    if let Some(entry) = &model.entry {
        draw_entry(frame, area, entry, model.display.color);
    }
}

/// The notice shown when the terminal is below the design size.
fn draw_too_small(frame: &mut Frame, area: Rect) {
    let notice = Paragraph::new(format!(
        "terminal too small (needs {MIN_WIDTH}x{MIN_HEIGHT})"
    ))
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    let row = Rect {
        y: area.y + area.height / 2,
        height: area.height.clamp(1, 2),
        ..area
    };
    frame.render_widget(notice, row);
}

/// The style of a tile with `feedback`: dark letters on green, yellow or grey.
const fn tile_style(feedback: Correctness) -> Style {
    let background = match feedback {
        Correctness::Correct => Color::Green,
        Correctness::Misplaced => Color::Yellow,
        Correctness::Absent => Color::DarkGray,
    };
    let foreground = match feedback {
        Correctness::Correct | Correctness::Misplaced => Color::Black,
        Correctness::Absent => Color::White,
    };
    Style::new().fg(foreground).bg(background)
}

/// The symbol a tile shows when colour is off.
const fn symbol(scheme: Symbols, feedback: Correctness) -> &'static str {
    match (scheme, feedback) {
        (Symbols::Letters, Correctness::Correct) => "g",
        (Symbols::Letters, Correctness::Misplaced) => "y",
        (Symbols::Letters, Correctness::Absent) => "x",
        (Symbols::Digits, Correctness::Correct) => "2",
        (Symbols::Digits, Correctness::Misplaced) => "1",
        (Symbols::Digits, Correctness::Absent) => "0",
        (Symbols::Emoji, Correctness::Correct) => "🟩",
        (Symbols::Emoji, Correctness::Misplaced) => "🟨",
        (Symbols::Emoji, Correctness::Absent) => "⬜",
    }
}

/// One played guess as a row of tiles.
fn tile_row(played: &Played, model: &Model) -> Line<'static> {
    let mut spans = Vec::new();
    for (letter, &feedback) in played.word.chars().zip(&played.feedback) {
        let letter: String = letter.to_uppercase().collect();
        if !spans.is_empty() {
            spans.push(Span::raw(" "));
        }
        if model.display.color {
            spans.push(Span::styled(format!(" {letter} "), tile_style(feedback)));
        } else {
            spans.push(Span::raw(format!(
                "{letter}{}",
                symbol(model.display.symbols, feedback)
            )));
        }
    }
    Line::from(spans)
}

/// The tiles, the target and where the game stands.
fn draw_board(frame: &mut Frame, area: Rect, model: &Model) {
    let target = match (&model.target, model.target_shown()) {
        (Some(target), true) => target.to_uppercase(),
        _ => vec!["?"; model.game.word_length].join(" "),
    };
    let block = Block::bordered().title(format!(" Target: {target} "));
    let mut lines: Vec<Line> = model
        .history
        .iter()
        .map(|played| tile_row(played, model))
        .collect();
    if let (Phase::Ready, Some(view)) = (model.phase, &model.explanation) {
        let next: Vec<String> = view
            .guess
            .chars()
            .map(|c| c.to_uppercase().to_string())
            .collect();
        lines.push(Line::styled(
            format!("{}  <- lex's guess", next.join(" ")),
            Style::new().add_modifier(Modifier::DIM),
        ));
    }
    let height = usize::from(block.inner(area).height);
    let hidden = lines.len().saturating_sub(height);
    let lines = lines.split_off(hidden);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// What lex weighed at this position, or where the game stands.
fn draw_explanation(frame: &mut Frame, area: Rect, model: &Model) {
    let block = Block::bordered().title(" lex's guess ");
    let lines = match (model.phase, &model.explanation) {
        (Phase::Starting, _) => vec![Line::raw("Drawing a target...")],
        (Phase::Thinking | Phase::Playing, _) => vec![Line::raw("Thinking...")],
        (Phase::Solved, _) => solved_lines(model),
        (Phase::Stuck, _) => vec![Line::raw(
            "No candidates remain: the target is not in lex's word list.",
        )],
        (Phase::Ready, Some(view)) => explanation_lines(view, model.display.details),
        (Phase::Ready, None) => Vec::new(),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The reveal of a solved game.
fn solved_lines(model: &Model) -> Vec<Line<'static>> {
    let guesses = model.history.len();
    let plural = if guesses == 1 { "" } else { "es" };
    let target = model.target.as_deref().unwrap_or_default().to_uppercase();
    vec![
        Line::raw(format!("Solved in {guesses} guess{plural}.")),
        Line::raw(format!("The answer was {target}.")),
        Line::raw(""),
        Line::raw("Press n for another word."),
    ]
}

/// The technique and the guesses weighed. The player view gives each guess's words left;
/// `details` adds the bits and every Guess cost, and names the opening book.
fn explanation_lines(view: &ExplanationView, details: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let technique = view
        .technique
        .as_deref()
        .map(|t| format!("chosen by the {t}"));
    let source = if details && view.from_book {
        " (from the opening book)"
    } else {
        ""
    };
    if let Some(technique) = technique {
        lines.push(Line::raw(format!("{technique}{source}")));
    }
    let plural = if view.candidates == 1 { "" } else { "s" };
    lines.push(Line::raw(format!(
        "{} possible answer{plural} left",
        view.candidates
    )));
    lines.push(Line::raw(""));
    for row in view.rows.iter().take(SHOWN) {
        let mut notes = Vec::new();
        if details {
            notes.push(format!("{:.3} bits", row.bits));
        }
        notes.push(format!("{:.1} words left", row.remaining));
        if let (true, Some(cost)) = (details, row.cost) {
            notes.push(format!("{cost:.2} guesses to finish"));
        }
        if !row.in_candidates {
            notes.push("not a possible answer".to_owned());
        }
        lines.push(Line::raw(format!("{}  ({})", row.word, notes.join(", "))));
    }
    lines
}

/// Two lines: where the game stands, and the keys.
fn draw_status(frame: &mut Frame, area: Rect, model: &Model) {
    let origin = match model.origin {
        Origin::Seed(seed) => format!("seed {seed}"),
        Origin::Typed => "typed".to_owned(),
    };
    let info = format!(
        " {} | {origin} | guesses {} | colour {} | symbols {}",
        model.game.name,
        model.history.len(),
        if model.display.color { "on" } else { "off" },
        model.display.symbols.name(),
    );
    let keys = " space play  n new  / type  r target  d details  c colour  s symbols  q quit";
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(info, Style::new().add_modifier(Modifier::BOLD)),
            Line::raw(keys),
        ]),
        area,
    );
}

/// The `/` box, over the middle of the screen.
fn draw_entry(frame: &mut Frame, area: Rect, entry: &Entry, color: bool) {
    let width = 44.min(area.width);
    let height = 6.min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let mut lines = vec![Line::raw(format!("> {}_", entry.text))];
    lines.push(match &entry.error {
        Some(error) if color => Line::styled(error.clone(), Style::new().fg(Color::Red)),
        Some(error) => Line::raw(error.clone()),
        None => Line::raw(""),
    });
    lines.push(Line::raw("enter: watch this word   esc: cancel"));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(" Type an answer "))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::fake::Fake;
    use crate::model::{Msg, Options};
    use crate::runtime::{drive, press};

    /// Colour is not asserted, so every snapshot runs with it off and the tiles carry symbols.
    const OPTIONS: Options = Options {
        seed: 0,
        color: false,
        symbols: Symbols::Letters,
    };

    /// `model` drawn on a terminal of `width` by `height`.
    fn render(model: &Model, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, model)).unwrap();
        terminal.backend().to_string()
    }

    /// The model after `keys`, against the fake solver, drawn at 80x24.
    fn screen(keys: &str) -> String {
        render(&drive(&mut Fake, OPTIONS, keys.chars().map(press)), 80, 24)
    }

    #[test]
    fn opening_screen() {
        insta::assert_snapshot!(screen(""));
    }

    #[test]
    fn mid_game() {
        insta::assert_snapshot!(screen("  "));
    }

    #[test]
    fn solved_with_the_target_revealed() {
        insta::assert_snapshot!(screen("    "));
    }

    #[test]
    fn details_view() {
        insta::assert_snapshot!(screen(" d"));
    }

    #[test]
    fn digits_for_the_symbols() {
        insta::assert_snapshot!(screen(" s"));
    }

    #[test]
    fn the_target_toggled_on_before_the_game_ends() {
        insta::assert_snapshot!(screen("r"));
    }

    #[test]
    fn word_entry_box_with_an_error() {
        let mut keys: Vec<_> = "/zebra".chars().map(press).collect();
        keys.push(Msg::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        )));
        let model = drive(&mut Fake, OPTIONS, keys);
        insta::assert_snapshot!(render(&model, 80, 24));
    }

    #[test]
    fn too_small_terminal() {
        let model = drive(&mut Fake, OPTIONS, []);
        insta::assert_snapshot!(render(&model, 79, 24));
        insta::assert_snapshot!(render(&model, 80, 23));
    }
}
