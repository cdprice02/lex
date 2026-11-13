//! `lex assist`: the interactive solver for a live game.

use std::io::{BufRead, Write};

use std::collections::HashSet;

use lex_core::{
    Correctness, Explanation, Guess, Guesser, ScoredGuess, WordCorrectness, answer_prior,
};
use lex_data::Word;

use crate::books::{self, BookMode};
use crate::config::Targets;
use crate::grading::{Grading, Worth, grading_line};
use crate::lists::Source;
use crate::progress;
use crate::style;

use crate::cli::AssistArgs;

/// Interactive helper for a live Wordle game.
///
/// Suggests guesses, reads the word the user actually played and the feedback Wordle returned,
/// and narrows the candidate set until solved (or until the candidates are exhausted, e.g. when
/// the day's target is missing from our corpus).
///
/// `args.symbols` sets how the echoed feedback is displayed; input is auto-detected whatever it is.
///
/// # Errors
///
/// If the lists or the Opening book cannot be loaded, or the terminal cannot be read.
pub fn assist<const N: usize>(args: &AssistArgs) -> anyhow::Result<()> {
    let config = args.common.config(Targets::Full);
    let data_dir = args.common.data();
    let (lists, source) = Source::load::<N>(&config, &data_dir)?;
    let guess_pool = lists.pool.clone();
    let mut survivors = args.details.then(|| {
        let answers: HashSet<Word<N>> = lists.answers.iter().copied().collect();
        answer_prior(&lists.pool, config.strategy.temperature)
            .into_iter()
            .filter(|(word, _)| answers.contains(word))
            .collect::<Vec<_>>()
    });
    let mut guesser = books::guesser(
        &config,
        &data_dir,
        &source,
        lists,
        BookMode::from_no_book(args.no_book),
        !args.no_explain,
        progress::observer().as_deref(),
    )?;

    guesser.set_explain(!args.no_explain);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    println!(
        "Assisting a {N}-letter game in {}. Feedback formats: letters (g=green, y=yellow, \
         x/b/./- = gray), digits (2=green, 1=yellow, 0=gray), or emoji.",
        source.name()
    );

    let mut turn = 1;
    loop {
        let num_candidates = guesser.num_candidates();
        if num_candidates == 0 {
            println!(
                "No candidates remain. A feedback entry may be inconsistent, or the target \
                 is not in the corpus."
            );
            return Ok(());
        }

        let (noun_suffix, verb_suffix) = if num_candidates == 1 {
            ("", "s")
        } else {
            ("s", "")
        };
        println!("\nTurn {turn}: {num_candidates} candidate{noun_suffix} remain{verb_suffix}");
        let (top, pick) = show_suggestions(&guesser, args.suggestions.max(1), args.details)?;

        let Some(word) = prompt_word::<N>(&mut lines, top, |w| {
            source.check_guess(&guess_pool, w)?;
            guesser.rule_violation(*w).map_or(Ok(()), Err)
        })?
        else {
            return Ok(()); // EOF
        };
        let Some(feedback) = prompt_feedback::<N>(&mut lines)? else {
            return Ok(()); // EOF
        };
        anstream::println!("  {word} -> {}", style::tiles(&*feedback, args.symbols));

        let graded = pick.and_then(|pick| {
            let played = guesser.score(word)?;
            Some((pick, played))
        });
        let line = |left: Option<usize>, realised_bits: Option<f64>| {
            graded.as_ref().map(|(pick, played)| {
                grading_line(
                    &Grading {
                        matched: pick.word == word,
                        played: worth(played),
                        pick: worth(pick),
                        left,
                        realised_bits,
                    },
                    args.details,
                )
            })
        };

        if feedback.is_correct() {
            if let Some(line) = line(None, None) {
                println!("{line}");
            }
            println!("Solved in {turn} turn{}!", if turn == 1 { "" } else { "s" });
            return Ok(());
        }
        let realised_bits = survivors
            .as_mut()
            .and_then(|set| narrow(set, word, feedback));
        guesser.push_guess(Guess::new(word, feedback));
        if let Some(line) = line(Some(guesser.num_candidates()), realised_bits) {
            println!("{line}");
        }
        turn += 1;
    }
}

/// The grading numbers of a scored guess.
const fn worth<const N: usize>(s: &ScoredGuess<N>) -> Worth {
    Worth {
        remaining: s.expected_remaining,
        bits: s.expected_bits,
        cost: s.guess_cost,
    }
}

/// Keeps the answers that earn `feedback` for `word` and returns the bits it gained, the
/// negative log of their share of the prior; `None` when none does.
fn narrow<const N: usize>(
    survivors: &mut Vec<(Word<N>, f64)>,
    word: Word<N>,
    feedback: WordCorrectness<N>,
) -> Option<f64> {
    let total: f64 = survivors.iter().map(|&(_, w)| w).sum();
    survivors.retain(|&(answer, _)| WordCorrectness::correct(answer, word) == feedback);
    let kept: f64 = survivors.iter().map(|&(_, w)| w).sum();
    (kept > 0.0).then(|| -(kept / total).log2())
}

/// Prints the guess `suggest` plays and, while the guesser explains, the technique that chose it
/// and up to `shown` of the guesses it weighed; returns the guess and the solver's scored pick.
///
/// The player view gives each guess's words left; `details` adds the bits and every Guess cost,
/// and names the opening book.
fn show_suggestions<const N: usize>(
    guesser: &Guesser<N>,
    shown: usize,
    details: bool,
) -> anyhow::Result<(Word<N>, Option<ScoredGuess<N>>)> {
    let Some(Explanation {
        guess,
        technique,
        from_book,
        guesses,
        ..
    }) = guesser.explain()
    else {
        let guess = guesser
            .suggest()
            .ok_or_else(|| anyhow::anyhow!("no valid guess"))?;
        println!("  {guess}");
        return Ok((guess, None));
    };
    let weighed: Vec<Weighed> = guesses
        .iter()
        .take(shown)
        .map(|s| Weighed {
            word: s.word.to_string(),
            bits: s.expected_bits,
            remaining: s.expected_remaining,
            cost: s.guess_cost,
            in_candidates: s.in_candidates,
        })
        .collect();
    print_weighed(
        &technique.to_string(),
        from_book && details,
        &weighed,
        details,
    );
    Ok((guess, guesses.first().copied()))
}

/// A guess the solver weighed, as `assist` prints it.
struct Weighed {
    /// The guess.
    word: String,
    /// Its expected information, in bits.
    bits: f64,
    /// Its Expected remaining.
    remaining: f64,
    /// Its Guess cost, when the Technique computed one.
    cost: Option<f64>,
    /// Whether it can still be the answer.
    in_candidates: bool,
}

/// Prints the Technique that chose the guess, `from_book` naming the opening book, and a line per
/// weighed guess. Not generic: the word length is already folded into [`Weighed`].
fn print_weighed(technique: &str, from_book: bool, weighed: &[Weighed], details: bool) {
    if from_book {
        println!("  chosen by the {technique} (from the opening book)");
    } else {
        println!("  chosen by the {technique}");
    }
    for w in weighed {
        let mut notes = Vec::new();
        if details {
            notes.push(format!("{:.3} bits", w.bits));
        }
        notes.push(format!("{:.1} words left", w.remaining));
        if let (true, Some(cost)) = (details, w.cost) {
            notes.push(format!("{cost:.2} guesses to finish"));
        }
        if !w.in_candidates {
            notes.push("not a possible answer".to_owned());
        }
        println!("  {}  ({})", w.word, notes.join(", "));
    }
}

/// Prompts for the word the user played. Empty input accepts `default`;
/// `Ok(None)` signals EOF. Re-prompts until the input parses as a `Word<N>` that `accept` allows.
fn prompt_word<const N: usize>(
    lines: &mut dyn Iterator<Item = std::io::Result<String>>,
    default: Word<N>,
    accept: impl Fn(&Word<N>) -> Result<(), String>,
) -> anyhow::Result<Option<Word<N>>> {
    loop {
        let Some(line) = prompt_line(lines, &format!("word played [{default}]: "))? else {
            return Ok(None);
        };
        let input = line.trim().to_lowercase();
        if input.is_empty() {
            return Ok(Some(default));
        }
        match Word::<N>::try_from(input.as_str()) {
            Ok(word) => match accept(&word) {
                Ok(()) => return Ok(Some(word)),
                Err(e) => println!("  {e}"),
            },
            Err(e) => println!("  {e}"),
        }
    }
}

/// Prompts for the feedback pattern. `Ok(None)` signals EOF. Re-prompts until
/// the input parses in one of the accepted formats.
fn prompt_feedback<const N: usize>(
    lines: &mut dyn Iterator<Item = std::io::Result<String>>,
) -> anyhow::Result<Option<WordCorrectness<N>>> {
    loop {
        let Some(line) = prompt_line(lines, "feedback (e.g. gyxxg / 21002 / 🟩🟨⬜⬜🟩): ")?
        else {
            return Ok(None);
        };
        match parse_feedback(&line, N) {
            Ok(symbols) => {
                let mut feedback = WordCorrectness::<N>::absent();
                feedback.copy_from_slice(&symbols);
                return Ok(Some(feedback));
            }
            Err(e) => println!("  {e}"),
        }
    }
}

/// Prints `prompt` and reads one line; `None` at EOF. The one read every prompt shares, so the
/// word length stays out of it.
fn prompt_line(
    lines: &mut dyn Iterator<Item = std::io::Result<String>>,
    prompt: &str,
) -> anyhow::Result<Option<String>> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    Ok(lines.next().transpose()?)
}

/// How a feedback pattern is written.
#[derive(Clone, Copy, PartialEq, Debug)]
enum FeedbackFormat {
    /// g / y / x (aliases for gray: b, `.`, `-`)
    Letters,
    /// 2 / 1 / 0 (matches the `Correctness` encoding)
    Digits,
    /// 🟩 / 🟨 / ⬜, including dark-mode (⬛) and high-contrast (🟧 / 🟦) variants
    Emoji,
}

/// Parses a feedback pattern of `n` symbols in any accepted format, auto-detected from the
/// first symbol. Formats must not be mixed within one entry.
fn parse_feedback(input: &str, n: usize) -> Result<Vec<Correctness>, String> {
    let symbols: Vec<char> = input.trim().chars().collect();
    if symbols.len() != n {
        return Err(format!("expected {n} symbols, got {}", symbols.len()));
    }

    let format = match symbols[0].to_ascii_lowercase() {
        'g' | 'y' | 'x' | 'b' | '.' | '-' => FeedbackFormat::Letters,
        '0' | '1' | '2' => FeedbackFormat::Digits,
        '🟩' | '🟨' | '⬜' | '⬛' | '🟧' | '🟦' => FeedbackFormat::Emoji,
        other => {
            return Err(format!(
                "unrecognized symbol {other:?}; use letters (gyx), digits (210), or emoji"
            ));
        }
    };

    let mut feedback = Vec::with_capacity(n);
    for (i, &symbol) in symbols.iter().enumerate() {
        feedback.push(match (format, symbol.to_ascii_lowercase()) {
            (FeedbackFormat::Letters, 'g')
            | (FeedbackFormat::Digits, '2')
            | (FeedbackFormat::Emoji, '🟩' | '🟧') => Correctness::Correct,
            (FeedbackFormat::Letters, 'y')
            | (FeedbackFormat::Digits, '1')
            | (FeedbackFormat::Emoji, '🟨' | '🟦') => Correctness::Misplaced,
            (FeedbackFormat::Letters, 'x' | 'b' | '.' | '-')
            | (FeedbackFormat::Digits, '0')
            | (FeedbackFormat::Emoji, '⬜' | '⬛') => Correctness::Absent,
            _ => {
                return Err(format!(
                    "symbol {symbol:?} at position {} doesn't match the {format:?} format \
                     detected from the first symbol",
                    i + 1
                ));
            }
        });
    }
    Ok(feedback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wc(pattern: [Correctness; 5]) -> WordCorrectness<5> {
        let mut wc = WordCorrectness::<5>::absent();
        for (i, c) in pattern.into_iter().enumerate() {
            wc[i] = c;
        }
        wc
    }

    /// `parse_feedback` at length `N`, as the prompt reads it.
    fn parse_feedback<const N: usize>(input: &str) -> Result<WordCorrectness<N>, String> {
        let symbols = super::parse_feedback(input, N)?;
        let mut feedback = WordCorrectness::<N>::absent();
        feedback.copy_from_slice(&symbols);
        Ok(feedback)
    }

    const G: Correctness = Correctness::Correct;
    const Y: Correctness = Correctness::Misplaced;
    const X: Correctness = Correctness::Absent;

    #[test]
    fn letters_mixed_case_and_aliases() {
        let expected = wc([G, Y, X, X, G]);
        assert_eq!(parse_feedback::<5>("gyxxg").unwrap(), expected);
        assert_eq!(parse_feedback::<5>("GYXXG").unwrap(), expected);
        assert_eq!(parse_feedback::<5>("gyb.G").unwrap(), expected);
        assert_eq!(parse_feedback::<5>("gy--g").unwrap(), expected);
    }

    #[test]
    fn digits_match_correctness_encoding() {
        assert_eq!(parse_feedback::<5>("21002").unwrap(), wc([G, Y, X, X, G]));
    }

    #[test]
    fn emoji_all_variants() {
        let expected = wc([G, Y, X, X, G]);
        assert_eq!(parse_feedback::<5>("🟩🟨⬜⬜🟩").unwrap(), expected);
        // dark mode grays
        assert_eq!(parse_feedback::<5>("🟩🟨⬛⬛🟩").unwrap(), expected);
        // high-contrast: orange = correct, blue = misplaced
        assert_eq!(parse_feedback::<5>("🟧🟦⬜⬜🟧").unwrap(), expected);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(parse_feedback::<5>("gyx").is_err());
        assert!(parse_feedback::<5>("gyxxgg").is_err());
        assert!(parse_feedback::<5>("").is_err());
    }

    #[test]
    fn rejects_mixed_formats() {
        assert!(parse_feedback::<5>("gy002").is_err());
        assert!(parse_feedback::<5>("21🟩🟨⬜").is_err());
    }

    #[test]
    fn rejects_unknown_symbols() {
        assert!(parse_feedback::<5>("gyzxg").is_err());
        assert!(parse_feedback::<5>("qwert").is_err());
    }

    #[test]
    fn whitespace_trimmed() {
        assert_eq!(
            parse_feedback::<5>("  ggggg\n").unwrap(),
            wc([G, G, G, G, G])
        );
    }

    #[test]
    fn all_correct_is_correct() {
        assert!(parse_feedback::<5>("22222").unwrap().is_correct());
        assert!(!parse_feedback::<5>("22221").unwrap().is_correct());
    }
}
