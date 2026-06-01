// TODO: add benches for important functions like cache read/write, word parsing, guesser next_guess, etc.
// TODO: add more comments and documentation for functions, structs, etc. to improve code readability and maintainability
// TODO: add more tests for edge cases, error handling, etc. (unit, integration, prop, etc.)
use std::path::PathBuf;

use clap::Parser;
use lex_data::Language;

mod error;
mod game;
mod guesser;

use error::LexError;
use game::play;

macro_rules! configure_word_length_bounds {
    ($min:literal, $max:literal) => {
        const MIN_WORD_LENGTH: usize = $min;
        const MAX_WORD_LENGTH: usize = $max;


    macro_rules! match_word_length_run {
                        ($args:expr; MIN_WORD_LENGTH..=MAX_WORD_LENGTH) => {
                            seq_macro::seq!(N in $min..=$max {
                                match $args.word_length {
                                    #(
                                        N => run::<N>($args),
                                    )*
                                    _ => unreachable!("parser enforces {MIN_WORD_LENGTH}..={MAX_WORD_LENGTH}"),
                                }
                            })
                        };
                    }
    };
}

configure_word_length_bounds!(3, 10);

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
        long,
        help = format!("Number of the words to allow in the guesser's dictionary (default: all words in the corpus)")
    )]
    dictionary_length: Option<usize>,
    #[arg(short = 'n', long, help = "Number of games to simulate")]
    num_games: Option<usize>,
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

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = Args::parse();

    match_word_length_run!(&args; MIN_WORD_LENGTH..=MAX_WORD_LENGTH)?;

    Ok(())
}

fn parse_word_length(s: &str) -> anyhow::Result<usize> {
    let n: usize = s.parse()?;
    if (MIN_WORD_LENGTH..=MAX_WORD_LENGTH).contains(&n) {
        Ok(n)
    } else {
        Err(LexError::UnexpectedWordLength {
            range: MIN_WORD_LENGTH..=MAX_WORD_LENGTH,
            got: n,
        }
        .into())
    }
}

fn run<const N: usize>(args: &Args) -> anyhow::Result<()> {
    // TODO: add word selection strategies (e.g. random, most/least frequent, etc.) instead of just taking the first n words
    let words = lex_data::blocking::get::<N>(&args.data_dir, args.lang, args.num_games)?.words();
    let num_words = words.len();

    log::info!(
        "Simulating {} games with {}-letter words in {}...",
        num_words,
        N,
        args.lang
    );

    let mut results = Vec::new();
    for word in words {
        let result = play(word, &args.data_dir, args.lang, args.dictionary_length)?;
        log::debug!("{}: {}", result.word(), result.num_guesses());
        results.push(result);
    }

    log::info!("Completed {} games", num_words);

    let avg_guesses =
        results.iter().map(|r| r.num_guesses() as f64).sum::<f64>() / num_words as f64;
    log::info!("Average number of guesses: {avg_guesses:.2}");

    Ok(())
}
