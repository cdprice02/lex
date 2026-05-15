use std::{collections::HashMap, fmt::Display, ops::Deref};

static WORDLE_WORDS_FILE_PATH: &str = "../data/wordle_words.txt";
static WORDLE_DICTIONARY_FILE_PATH: &str = "../data/wordle_dictionary.txt";
static WORD_FREQUENCY_FILE_PATH: &str = "../data/words_with_frequencies.csv";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Word<const N: usize>([char; N]);

impl<const N: usize> Word<N> {
    fn matches(&self, guess: &Guess<N>) -> bool {
        let correctness = WordCorrectness::correct(*self, guess.word);
        correctness == guess.correctness
    }
}

impl<const N: usize> From<&str> for Word<N> {
    fn from(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        assert!(
            chars.len() == N,
            "input string must have exactly {} characters",
            N
        );
        let array: [char; N] = chars.try_into().expect("length checked above");
        Word(array)
    }
}

impl<const N: usize> Deref for Word<N> {
    type Target = [char; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const N: usize> Display for Word<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for ch in &self.0 {
            write!(f, "{}", ch)?;
        }
        Ok(())
    }
}

impl<const N: usize> IntoIterator for Word<N> {
    type Item = char;
    type IntoIter = std::array::IntoIter<char, N>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIterator::into_iter(*self)
    }
}

#[derive(Debug, Clone)]
struct Guess<const N: usize> {
    word: Word<N>,
    correctness: WordCorrectness<N>,
}

impl<const N: usize> Guess<N> {
    pub fn new(word: Word<N>, correctness: WordCorrectness<N>) -> Self {
        Self { word, correctness }
    }
}

struct GameResult<const N: usize> {
    word: Word<N>,
    guesses: Vec<Guess<N>>,
}

impl<const N: usize> GameResult<N> {
    pub fn new(word: Word<N>) -> Self {
        Self {
            word,
            guesses: Vec::new(),
        }
    }

    pub fn add_guess(&mut self, guess: Guess<N>) {
        self.guesses.push(guess);
    }

    pub fn word(&self) -> Word<N> {
        self.word
    }

    pub fn num_guesses(&self) -> usize {
        self.guesses.len()
    }
}

fn main() {
    let wordle_words = load_wordle_words();
    let num_words = wordle_words.len();

    let results = wordle_words.into_iter().map(|word| {
        let word = Word::<5>(
            word.chars()
                .collect::<Vec<_>>()
                .try_into()
                .expect("wordle words are all 5 letters"),
        );
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

fn load_wordle_words() -> Vec<String> {
    let contents =
        std::fs::read_to_string(WORDLE_WORDS_FILE_PATH).expect("Failed to read Wordle words file");
    contents.lines().map(|line| line.to_string()).collect()
}

fn load_wordle_dictionary() -> Vec<String> {
    let contents = std::fs::read_to_string(WORDLE_DICTIONARY_FILE_PATH)
        .expect("Failed to read Wordle dictionary file");
    contents.lines().map(|line| line.to_string()).collect()
}

fn load_word_frequencies() -> Vec<(String, usize)> {
    let contents = std::fs::read_to_string(WORD_FREQUENCY_FILE_PATH)
        .expect("Failed to read word frequency file");
    contents
        .lines()
        .filter_map(|line| {
            let (word, freq) = line.split_once(',')?;
            let word = word.to_string();
            let freq = freq.parse().ok()?;

            Some((word, freq))
        })
        .collect()
}

fn play<const N: usize>(word: Word<N>) -> GameResult<N> {
    let mut result = GameResult::new(word);

    let mut guesser = Guesser::<N>::new();
    let first_word = Word::from("trace"); // TODO: use guesser to get the first guess; right now it is too complex of a problem
    let guess = Guess::<N>::new(first_word, WordCorrectness::correct(word, first_word));
    result.add_guess(guess);
    let mut is_correct = result
        .guesses
        .last()
        .expect("just added guess")
        .correctness
        .is_correct();
    while !is_correct {
        let word = guesser.next_guess(result.guesses.clone());
        let guess = Guess::<N>::new(word, WordCorrectness::correct(result.word(), word));
        result.add_guess(guess);
        eprintln!(
            "Guess {}: {} -> {}",
            result.num_guesses(),
            result.guesses.last().unwrap().word,
            result.guesses.last().unwrap().correctness
        );
        is_correct = result
            .guesses
            .last()
            .expect("just added guess")
            .correctness
            .is_correct();
    }

    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Correctness {
    Absent,
    Misplaced,
    Correct,
}

impl std::fmt::Display for Correctness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let symbol = match self {
            Correctness::Absent => '⬜',
            Correctness::Misplaced => '🟨',
            Correctness::Correct => '🟩',
        };
        write!(f, "{}", symbol)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WordCorrectness<const N: usize>([Correctness; N]);

impl<const N: usize> WordCorrectness<N> {
    fn absent() -> Self {
        Self([Correctness::Absent; N])
    }

    fn correct(word: Word<N>, guess: Word<N>) -> Self {
        let mut result = Self::absent();
        let mut used = [false; N];

        word.iter()
            .zip(guess)
            .enumerate()
            .for_each(|(i, (w_ch, g_ch))| {
                if *w_ch == g_ch {
                    result.0[i] = Correctness::Correct;
                    used[i] = true;
                } else if word.contains(&g_ch) {
                    result.0[i] = Correctness::Misplaced;
                } else {
                    result.0[i] = Correctness::Absent;
                    used[i] = true;
                }
            });

        for (i, g_ch) in guess.iter().enumerate() {
            if result.0[i] == Correctness::Misplaced {
                let mut found = false;
                for (j, w_ch) in word.iter().enumerate() {
                    if !used[j] && w_ch == g_ch {
                        used[j] = true;
                        found = true;
                        break;
                    }
                }
                if !found {
                    result.0[i] = Correctness::Absent;
                }
            }
        }

        result
    }

    fn is_correct(&self) -> bool {
        self.0.iter().all(|&c| c == Correctness::Correct)
    }
}

impl<const N: usize> std::fmt::Display for WordCorrectness<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for state in &self.0 {
            write!(f, "{}", state)?;
        }
        Ok(())
    }
}

struct Guesser<const N: usize> {
    dictionary: Vec<Word<N>>,
    word_probabilities: HashMap<Word<N>, f64>,
}

impl<const N: usize> Guesser<N> {
    pub fn new() -> Self {
        let dictionary = load_wordle_dictionary()
            .into_iter()
            .map(|word| Word::from(word.as_str()))
            .collect();

        let word_frequencies: HashMap<Word<N>, usize> = load_word_frequencies()
            .into_iter()
            .map(|(word, freq)| (Word::from(word.as_str()), freq))
            .collect();

        let total_frequency: usize = word_frequencies.values().sum();

        let word_probabilities: HashMap<Word<N>, f64> = word_frequencies
            .iter()
            .map(|(word, freq)| (*word, *freq as f64 / total_frequency as f64))
            .collect();

        Self {
            dictionary,
            word_probabilities,
        }
    }

    pub fn next_guess(&mut self, history: Vec<Guess<N>>) -> Word<N> {
        let last_guess = history
            .last()
            .expect("history should have at least one guess"); // TODO: allow guessing the first word

        self.dictionary.retain(|w| w.matches(last_guess));
        return self.dictionary[20.min(self.dictionary.len() - 1)]; // TODO: use probabilities to pick the best guess, not just the first one
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_correct() {
        let word = Word::from("crate");
        let guess = Word::from("crate");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_correct_short() {
        let word = Word::from("ace");
        let guess = Word::from("ace");
        let correctness = WordCorrectness::<3>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_incorrect() {
        let word = Word::from("crate");
        let guess = Word::from("fling");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent
            ])
        );
    }
    #[test]
    fn test_incorrect_short() {
        let word = Word::from("ace");
        let guess = Word::from("bug");
        let correctness = WordCorrectness::<3>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Absent,
            ])
        );
    }

    #[test]
    fn test_partial_1() {
        let word = Word::from("crate");
        let guess = Word::from("trace");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Misplaced,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_partial_2() {
        let word = Word::from("train");
        let guess = Word::from("trina");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Misplaced,
                Correctness::Misplaced
            ])
        );
    }

    #[test]
    fn test_mixed_1() {
        let word = Word::from("apple");
        let guess = Word::from("allee");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Misplaced,
                Correctness::Absent,
                Correctness::Absent,
                Correctness::Correct
            ])
        );
    }

    #[test]
    fn test_mixed_2() {
        let word = Word::from("stats");
        let guess = Word::from("state");
        let correctness = WordCorrectness::<5>::correct(word, guess);
        assert_eq!(
            correctness,
            WordCorrectness([
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Correct,
                Correctness::Absent
            ])
        );
    }
}
