use std::{fmt::Display, ops::Deref};

static WORDLE_WORDS_FILE_PATH: &str = "../data/wordle.txt";
static WORD_FREQUENCY_FILE_PATH: &str = "../data/words_with_frequencies.csv";

#[derive(Debug, Clone, Copy)]
struct Word<const N: usize>([char; N]);

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

struct GameResult<const N: usize> {
    word: Word<N>,
    guesses: Vec<Word<N>>,
    correctness: Vec<WordCorrectness<N>>,
}

impl<const N: usize> GameResult<N> {
    fn new(word: Word<N>) -> Self {
        Self {
            word,
            guesses: Vec::new(),
            correctness: Vec::new(),
        }
    }

    fn add_guess(&mut self, guess: Word<N>, correctness: WordCorrectness<N>) {
        self.guesses.push(guess);
        self.correctness.push(correctness);
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
    let word_frequencies = load_word_frequencies();

    for word in wordle_words {
        let word = Word::<5>(
            word.chars()
                .collect::<Vec<_>>()
                .try_into()
                .expect("wordle words are all 5 letters"),
        );
        let result = play(word);
        println!("Guessed word {} in {}", result.word(), result.num_guesses());
    }
}

fn load_wordle_words() -> Vec<String> {
    let contents =
        std::fs::read_to_string(WORDLE_WORDS_FILE_PATH).expect("Failed to read Wordle words file");
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
    println!("Playing with word: {}", word);

    let mut result = GameResult::new(word);

    let guess = get_guess();
    let mut correctness = WordCorrectness::<N>::correct(word, guess);
    while !correctness.is_correct() {
        println!("guessed: {}, correctness: {}", guess, correctness);
        result.add_guess(guess, correctness);
        let guess = get_guess();
        correctness = WordCorrectness::<N>::correct(word, guess);
    }

    result
}

fn get_guess<const N: usize>() -> Word<N> {
    todo!()
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
