mod data;
mod game;
mod guesser;
mod word;

use data::load_wordle_words;
use game::play;

fn main() {
    let wordle_words = load_wordle_words::<5>();
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
