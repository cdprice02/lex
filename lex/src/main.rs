// TODO: introduce log crate to replace eprintln! for better log management (e.g. log levels, output to file, etc.)
// TODO: add benches for important functions like cache read/write, word parsing, guesser next_guess, etc.
// TODO: add more error handling and propagate errors instead of panicking (e.g. when loading cache, parsing words, etc.)
// TODO: add more comments and documentation for functions, structs, etc. to improve code readability and maintainability
// TODO: add more tests for edge cases, error handling, etc. (unit, integration, prop, etc.)
use std::path::PathBuf;

use clap::Parser;
use lex_data::Language;

mod game;
mod guesser;

use game::play;

const MIN_WORD_LENGTH: usize = 3;
const MAX_WORD_LENGTH: usize = 10;

// TODO: implement subcommands for different modes (e.g. interactive mode, simulation mode, etc.)
// TODO: implement subcommands for listing words, frequencies, etc. for debugging and exploration purposes
// TODO: implement subcommands for listing options of parameters like language, word length, etc.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short = 'l',
        long,
        default_value = "5",
        value_parser = parse_word_length,
        help = format!("Length of the words to use in the game ({MIN_WORD_LENGTH}–{MAX_WORD_LENGTH})")
    )]
    word_length: usize,
    #[arg(
        short = 'n',
        long,
        default_value = "0",
        help = "Number of games to simulate (0 for all)"
    )]
    num_games: usize,
    #[arg(
        long,
        default_value = "english",
        help = "Language corpus (english/eng, french/fre, german/ger, spanish/spa, italian/ita, russian/rus)"
    )]
    lang: Language,
    #[arg(
        long,
        default_value = "data/ngrams",
        help = "Directory containing cached word-frequency files"
    )]
    data_dir: PathBuf,
}

fn main() {
    let args = Args::parse();
    match args.word_length {
        3 => run::<3>(&args),
        4 => run::<4>(&args),
        5 => run::<5>(&args),
        6 => run::<6>(&args),
        7 => run::<7>(&args),
        8 => run::<8>(&args),
        9 => run::<9>(&args),
        10 => run::<10>(&args),
        _ => unreachable!("parser enforces {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}"),
    }
}

fn parse_word_length(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid number"))?;
    if (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&n) {
        Ok(n)
    } else {
        Err(format!(
            "word length must be in [{MIN_WORD_LENGTH}, {MAX_WORD_LENGTH}], got {n}"
        ))
    }
}

fn run<const N: usize>(args: &Args) {
    let words = lex_data::blocking::get::<N>(&args.data_dir, args.lang)
        .expect("failed to load word data")
        .words();
    let words = if args.num_games == 0 {
        words
    } else {
        // TODO: add word selection strategies (e.g. random, most/least frequent, etc.) instead of just taking the first n words
        words.into_iter().take(args.num_games).collect()
    };
    let num_words = words.len();

    let results: Vec<_> = words
        .into_iter()
        .map(|word| {
            let result = play(word, &args.data_dir, args.lang);
            println!("{}: {}", result.word(), result.num_guesses());
            result
        })
        .collect();

    let avg_guesses =
        results.iter().map(|r| r.num_guesses() as f64).sum::<f64>() / num_words as f64;
    println!("Average number of guesses: {avg_guesses:.2}");
}
