//! Terminal styling shared by every frontend: one palette, feedback tiles in three symbol
//! schemes, and the colour policy. Nothing here knows about a particular subcommand.

use std::fmt::Write as _;

use anstream::ColorChoice;
use anstyle::{AnsiColor, Style};
use clap::ValueEnum;
use lex_core::{Correctness, WordCorrectness};

/// When to emit ANSI colour (`--color`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorArg {
    /// Colour on a terminal, honouring `NO_COLOR` and `CLICOLOR`
    #[default]
    Auto,
    /// Always colour, even when piped
    Always,
    /// Never colour
    Never,
}

impl From<ColorArg> for ColorChoice {
    fn from(arg: ColorArg) -> Self {
        match arg {
            ColorArg::Auto => Self::Auto,
            ColorArg::Always => Self::Always,
            ColorArg::Never => Self::Never,
        }
    }
}

/// How feedback tiles are written for display (`--symbols`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum SymbolScheme {
    /// `g` / `y` / `x`
    #[default]
    Letters,
    /// `2` / `1` / `0`, the `Correctness` encoding
    Digits,
    /// Coloured square emoji
    Emoji,
}

/// Installs the colour policy for every `anstream` print in the process.
///
/// `anstream` strips escape codes from its output unless the policy and the stream allow them, so
/// callers write styled text without checking the terminal themselves.
pub fn init(color: ColorArg) {
    ColorChoice::write_global(color.into());
}

/// The palette: the style of a tile for each kind of feedback. Emoji tiles carry their own colour.
#[must_use]
pub const fn tile_style(feedback: Correctness) -> Style {
    let (fg, bg) = match feedback {
        Correctness::Correct => (AnsiColor::Black, AnsiColor::Green),
        Correctness::Misplaced => (AnsiColor::Black, AnsiColor::Yellow),
        Correctness::Absent => (AnsiColor::White, AnsiColor::BrightBlack),
    };
    Style::new()
        .fg_color(Some(anstyle::Color::Ansi(fg)))
        .bg_color(Some(anstyle::Color::Ansi(bg)))
}

/// Text that went right: a histogram bar within the guess limit. The Correct tile's green, as
/// a foreground.
#[must_use]
pub const fn success_style() -> Style {
    Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Green)))
}

/// Text that went wrong: a failed game's histogram row.
#[must_use]
pub const fn failure_style() -> Style {
    Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::Red)))
}

/// Text of secondary importance: provenance lines.
#[must_use]
pub const fn dim_style() -> Style {
    Style::new().dimmed()
}

/// Text that carries the headline number.
#[must_use]
pub const fn emphasis_style() -> Style {
    Style::new().bold()
}

/// The symbol of one tile in `scheme`.
#[must_use]
pub const fn symbol(scheme: SymbolScheme, feedback: Correctness) -> &'static str {
    match (scheme, feedback) {
        (SymbolScheme::Letters, Correctness::Correct) => "g",
        (SymbolScheme::Letters, Correctness::Misplaced) => "y",
        (SymbolScheme::Letters, Correctness::Absent) => "x",
        (SymbolScheme::Digits, Correctness::Correct) => "2",
        (SymbolScheme::Digits, Correctness::Misplaced) => "1",
        (SymbolScheme::Digits, Correctness::Absent) => "0",
        (SymbolScheme::Emoji, Correctness::Correct) => "🟩",
        (SymbolScheme::Emoji, Correctness::Misplaced) => "🟨",
        (SymbolScheme::Emoji, Correctness::Absent) => "⬜",
    }
}

/// Renders a feedback pattern as tiles. Letter and digit tiles are padded and coloured; emoji
/// tiles are left bare. The result holds escape codes, which `anstream` removes when colour is off.
#[must_use]
pub fn tiles<const N: usize>(feedback: &WordCorrectness<N>, scheme: SymbolScheme) -> String {
    let mut out = String::new();
    for i in 0..N {
        let state = feedback[i];
        let sym = symbol(scheme, state);
        if scheme == SymbolScheme::Emoji {
            out.push_str(sym);
        } else {
            let style = tile_style(state);
            // Writing to a String cannot fail.
            let _ = write!(out, "{style} {sym} {style:#}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use anstream::AutoStream;

    use super::*;

    fn pattern() -> WordCorrectness<5> {
        let mut wc = WordCorrectness::<5>::absent();
        wc[0] = Correctness::Correct;
        wc[1] = Correctness::Misplaced;
        wc[4] = Correctness::Correct;
        wc
    }

    fn plain(s: &str) -> String {
        anstream::adapter::strip_str(s).to_string()
    }

    #[test]
    fn letters_tiles() {
        assert_eq!(
            plain(&tiles(&pattern(), SymbolScheme::Letters)),
            " g  y  x  x  g "
        );
    }

    #[test]
    fn digits_tiles() {
        assert_eq!(
            plain(&tiles(&pattern(), SymbolScheme::Digits)),
            " 2  1  0  0  2 "
        );
    }

    #[test]
    fn emoji_tiles_are_bare() {
        let rendered = tiles(&pattern(), SymbolScheme::Emoji);
        assert_eq!(rendered, "🟩🟨⬜⬜🟩");
    }

    #[test]
    fn coloured_tiles_carry_escapes_and_reset() {
        let rendered = tiles(&pattern(), SymbolScheme::Letters);
        assert!(rendered.contains('\x1b'));
        assert!(rendered.ends_with("\x1b[0m"));
    }

    #[test]
    fn palette_distinguishes_the_three_states() {
        let styles = [
            tile_style(Correctness::Correct),
            tile_style(Correctness::Misplaced),
            tile_style(Correctness::Absent),
        ];
        assert_ne!(styles[0], styles[1]);
        assert_ne!(styles[1], styles[2]);
        assert_ne!(styles[0], styles[2]);
    }

    #[test]
    fn success_is_the_correct_tile_green() {
        assert_eq!(
            success_style().get_fg_color(),
            tile_style(Correctness::Correct).get_bg_color()
        );
    }

    #[test]
    fn color_arg_maps_to_choice() {
        assert_eq!(ColorChoice::from(ColorArg::Auto), ColorChoice::Auto);
        assert_eq!(ColorChoice::from(ColorArg::Always), ColorChoice::Always);
        assert_eq!(ColorChoice::from(ColorArg::Never), ColorChoice::Never);
    }

    fn through(choice: ColorChoice, text: &str) -> String {
        let mut stream = AutoStream::new(Vec::new(), choice);
        stream.write_all(text.as_bytes()).unwrap();
        String::from_utf8(stream.into_inner()).unwrap()
    }

    #[test]
    fn never_strips_and_always_keeps_escapes() {
        let rendered = tiles(&pattern(), SymbolScheme::Letters);
        assert!(!through(ColorChoice::Never, &rendered).contains('\x1b'));
        assert!(through(ColorChoice::Always, &rendered).contains('\x1b'));
    }

    #[test]
    fn auto_on_a_pipe_strips_escapes() {
        // A Vec is not a terminal, so Auto resolves to no colour.
        let rendered = tiles(&pattern(), SymbolScheme::Letters);
        assert!(!through(ColorChoice::Auto, &rendered).contains('\x1b'));
    }
}
