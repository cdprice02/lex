static WORDLE_WORDS_FILE_PATH: &str = "../data/wordle.txt";
static WORD_FREQUENCY_FILE_PATH: &str = "../data/words_with_frequencies.csv";

fn main() {
    let wordle_words = load_wordle_words();
    let word_frequencies = load_word_frequencies();

    let word = wordle_words[0].clone();
    println!("word: {}", word);

    let guess = "crate".to_string();

    assert!(word.len() == 5);
    assert!(guess.len() == word.len());
    let correctness = WordCorrectness::<5>::correct(&word, &guess);
    println!("guessed: {}, correctness: {}", guess, correctness);
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

    fn correct(word: &str, guess: &str) -> Self {
        assert!(word.len() == N);
        assert!(guess.len() == N);

        let mut result = Self::absent();
        let mut used = [false; N];

        for (i, (w_ch, g_ch)) in word.chars().zip(guess.chars()).enumerate() {
            if w_ch == g_ch {
                result.0[i] = Correctness::Correct;
                used[i] = true;
            } else if word.contains(g_ch) {
                result.0[i] = Correctness::Misplaced;
            } else {
                result.0[i] = Correctness::Absent;
                used[i] = true;
            }
        }

        for (i, g_ch) in guess.chars().enumerate() {
            if result.0[i] == Correctness::Misplaced {
                let mut found = false;
                for (j, w_ch) in word.chars().enumerate() {
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
        let word = "crate";
        let guess = "crate";
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
        let word = "ace";
        let guess = "ace";
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
        let word = "crate";
        let guess = "fling";
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
        let word = "ace";
        let guess = "bug";
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
        let word = "crate";
        let guess = "trace";
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
        let word = "train";
        let guess = "trina";
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
        let word = "apple";
        let guess = "allee";
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
        let word = "stats";
        let guess = "state";
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
