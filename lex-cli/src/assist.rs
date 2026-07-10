use std::io::{BufRead, Write};

use lex_core::{Correctness, Guess, Guesser, WordCorrectness};
use lex_data::Word;

use crate::cli::AssistArgs;

/// Interactive helper for a live Wordle game: suggests guesses, reads the word
/// the user actually played and the feedback Wordle returned, and narrows the
/// candidate set until solved (or until the candidates are exhausted, e.g.
/// when the day's target is missing from our corpus).
pub fn assist<const N: usize>(args: &AssistArgs) -> anyhow::Result<()> {
    let word_set = lex_data::blocking::DataDir::new(&args.common.data_dir)
        .load::<N>(args.common.lang, args.common.dictionary_length)?;
    let mut guesser = Guesser::new(word_set);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    println!(
        "Assisting a {N}-letter game in {}. Feedback formats: letters (g=green, y=yellow, \
         x/b/./- = gray), digits (2=green, 1=yellow, 0=gray), or emoji.",
        args.common.lang
    );

    let mut turn = 1;
    loop {
        let num_candidates = guesser.num_candidates();
        if num_candidates == 0 {
            println!(
                "No candidates remain — a feedback entry may be inconsistent, or the target \
                 is not in the corpus."
            );
            return Ok(());
        }

        let (noun_suffix, verb_suffix) = if num_candidates == 1 {
            ("", "s")
        } else {
            ("s", "")
        };
        println!("\nTurn {turn} — {num_candidates} candidate{noun_suffix} remain{verb_suffix}");
        let suggestions = guesser.suggest_top_k(args.suggestions.max(1));
        for (word, bits) in &suggestions {
            println!("  {word}  ({bits:.3} bits)");
        }
        let top = suggestions[0].0;

        let Some(word) = prompt_word::<N>(&mut lines, top)? else {
            return Ok(()); // EOF
        };
        let Some(feedback) = prompt_feedback::<N>(&mut lines)? else {
            return Ok(()); // EOF
        };
        println!("  {word} -> {feedback}");

        if feedback.is_correct() {
            println!("Solved in {turn} turn{}!", if turn == 1 { "" } else { "s" });
            return Ok(());
        }
        guesser.push_guess(Guess::new(word, feedback));
        turn += 1;
    }
}

/// Prompts for the word the user played. Empty input accepts `default`;
/// `Ok(None)` signals EOF. Re-prompts until the input parses as a `Word<N>`.
fn prompt_word<const N: usize>(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    default: Word<N>,
) -> anyhow::Result<Option<Word<N>>> {
    loop {
        print!("word played [{default}]: ");
        std::io::stdout().flush()?;
        let Some(line) = lines.next().transpose()? else {
            return Ok(None);
        };
        let input = line.trim().to_lowercase();
        if input.is_empty() {
            return Ok(Some(default));
        }
        match Word::<N>::try_from(input.as_str()) {
            Ok(word) => return Ok(Some(word)),
            Err(e) => println!("  {e}"),
        }
    }
}

/// Prompts for the feedback pattern. `Ok(None)` signals EOF. Re-prompts until
/// the input parses in one of the accepted formats.
fn prompt_feedback<const N: usize>(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
) -> anyhow::Result<Option<WordCorrectness<N>>> {
    loop {
        print!("feedback (e.g. gyxxg / 21002 / 🟩🟨⬜⬜🟩): ");
        std::io::stdout().flush()?;
        let Some(line) = lines.next().transpose()? else {
            return Ok(None);
        };
        match parse_feedback::<N>(&line) {
            Ok(feedback) => return Ok(Some(feedback)),
            Err(e) => println!("  {e}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum FeedbackFormat {
    /// g / y / x (aliases for gray: b, `.`, `-`)
    Letters,
    /// 2 / 1 / 0 (matches the `Correctness` encoding)
    Digits,
    /// 🟩 / 🟨 / ⬜, including dark-mode (⬛) and high-contrast (🟧 / 🟦) variants
    Emoji,
}

/// Parses a feedback pattern in any accepted format, auto-detected from the
/// first symbol. Formats must not be mixed within one entry.
fn parse_feedback<const N: usize>(input: &str) -> Result<WordCorrectness<N>, String> {
    let symbols: Vec<char> = input.trim().chars().collect();
    if symbols.len() != N {
        return Err(format!("expected {N} symbols, got {}", symbols.len()));
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

    let mut feedback = WordCorrectness::<N>::absent();
    for (i, &symbol) in symbols.iter().enumerate() {
        feedback[i] = match (format, symbol.to_ascii_lowercase()) {
            (FeedbackFormat::Letters, 'g') => Correctness::Correct,
            (FeedbackFormat::Letters, 'y') => Correctness::Misplaced,
            (FeedbackFormat::Letters, 'x' | 'b' | '.' | '-') => Correctness::Absent,
            (FeedbackFormat::Digits, '2') => Correctness::Correct,
            (FeedbackFormat::Digits, '1') => Correctness::Misplaced,
            (FeedbackFormat::Digits, '0') => Correctness::Absent,
            (FeedbackFormat::Emoji, '🟩' | '🟧') => Correctness::Correct,
            (FeedbackFormat::Emoji, '🟨' | '🟦') => Correctness::Misplaced,
            (FeedbackFormat::Emoji, '⬜' | '⬛') => Correctness::Absent,
            _ => {
                return Err(format!(
                    "symbol {symbol:?} at position {} doesn't match the {format:?} format \
                     detected from the first symbol",
                    i + 1
                ));
            }
        };
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
