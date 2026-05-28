use clap::Parser;

mod data;
mod game;
mod guesser;
mod word;

use data::load_wordle_words;
use game::play;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(
        short = 'l',
        long,
        default_value = "5",
        help = "Length of the words to use in the game"
    )]
    word_length: usize,
    #[arg(
        short = 'n',
        long,
        default_value = "0",
        help = "Number of games to simulate (0 for all)"
    )]
    num_games: usize,
}

fn main() {
    let args = Args::parse();

    // TODO: move gram project to module within lex
    let wordle_words = load_wordle_words::<5>(); // TODO: use args.word_length
    let wordle_words = if args.num_games == 0 {
        wordle_words
    } else {
        wordle_words.into_iter().take(args.num_games).collect()
    };
    let num_words = wordle_words.len();

    let results = wordle_words.into_iter().map(|word| {
        let result = play(word);
        println!("{}: {}", result.word(), result.num_guesses());
        result
    });

    let avg_guesses = results
        .map(|result| result.num_guesses() as f64)
        .sum::<f64>()
        / (num_words as f64);
    println!("Average number of guesses: {:.2}", avg_guesses);
}
