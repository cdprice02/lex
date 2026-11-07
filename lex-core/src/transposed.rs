//! The Candidate set transposed for the entropy scan: one contiguous byte column per letter
//! position, and a letter-presence mask per word, scored against a guess a block of words at a
//! time. The kernel yields exactly [`pattern_ordinal`](crate::correctness::pattern_ordinal).

use crate::correctness::Code;

/// Words scored together. A multiple of every vector width the target may use, and small enough
/// that a block's working set stays in the L1 cache at the longest word length.
pub const BLOCK: usize = 64;

/// Bits in one presence word.
const PRESENCE_BITS: usize = 64;

/// One block of a column of letter codes.
type ByteBlock = [u8; BLOCK];

/// One block of a presence word per word of the set.
type MaskBlock = [u64; BLOCK];

/// Presence for a letter no word of the set can hold.
static NO_PRESENCE: MaskBlock = [0; BLOCK];

/// The words of a Candidate set as `N` letter columns plus the letters each word contains, padded
/// with words that match nothing to a whole number of [`BLOCK`]s.
pub struct Columns<const N: usize> {
    /// Number of real words.
    len: usize,
    /// `cols[i][k][b]` is letter `i` of word `k * BLOCK + b`; padding is `UNKNOWN_LETTER`.
    cols: [Vec<ByteBlock>; N],
    /// `presence[w][k][b]` has bit `c % 64` set when word `k * BLOCK + b` holds the letter with
    /// code `c`, for `c / 64 == w`. The Alphabet is at most 255 letters, so up to four words.
    presence: Vec<Vec<MaskBlock>>,
}

impl<const N: usize> Columns<N> {
    /// Gathers `words` in order. `alphabet_len` bounds every code in `words`.
    pub fn gather<'a>(
        words: impl ExactSizeIterator<Item = &'a Code<N>>,
        alphabet_len: usize,
    ) -> Self {
        let len = words.len();
        let blocks = len.div_ceil(BLOCK);
        let mut cols: [Vec<ByteBlock>; N] =
            std::array::from_fn(|_| vec![[crate::correctness::UNKNOWN_LETTER; BLOCK]; blocks]);
        let mut presence =
            vec![vec![[0_u64; BLOCK]; blocks]; alphabet_len.div_ceil(PRESENCE_BITS).max(1)];
        for (at, word) in words.enumerate() {
            let (k, b) = (at / BLOCK, at % BLOCK);
            for (col, &code) in cols.iter_mut().zip(word) {
                col[k][b] = code;
                let letter = usize::from(code);
                if let Some(slot) = presence.get_mut(letter / PRESENCE_BITS) {
                    slot[k][b] |= 1 << (letter % PRESENCE_BITS);
                }
            }
        }
        Self {
            len,
            cols,
            presence,
        }
    }

    /// Number of blocks, the last possibly padded.
    pub const fn blocks(&self) -> usize {
        self.len.div_ceil(BLOCK)
    }

    /// Real words in block `k`.
    pub const fn block_len(&self, k: usize) -> usize {
        let rest = self.len - k * BLOCK;
        if rest < BLOCK { rest } else { BLOCK }
    }

    /// The feedback ordinal of each word of block `k` against the guess of `plan`; entries past
    /// [`block_len`](Self::block_len) are meaningless.
    pub fn ordinals(&self, plan: &Plan<N>, k: usize, out: &mut [u64; BLOCK]) {
        let mut green = [[0_u8; BLOCK]; N];
        for ((green, col), &letter) in green.iter_mut().zip(&self.cols).zip(&plan.guess) {
            for (g, &c) in green.iter_mut().zip(&col[k]) {
                *g = u8::from(c == letter);
            }
        }
        let mut digit = [[0_u8; BLOCK]; N];
        for &Once { at, word, shift } in plan.once() {
            let present = self.presence.get(word).map_or(&NO_PRESENCE, |p| &p[k]);
            for ((d, &g), &mask) in digit[at].iter_mut().zip(&green[at]).zip(present) {
                let yellow = u8::from(mask >> shift & 1 == 1);
                *d = if g == 1 { 2 } else { yellow };
            }
        }
        for group in plan.groups() {
            // word letters equal to the guess letter that no green already matched
            let mut unmatched = [0_u8; BLOCK];
            for col in &self.cols {
                for (u, &c) in unmatched.iter_mut().zip(&col[k]) {
                    *u += u8::from(c == group.letter);
                }
            }
            for &at in group.positions() {
                for (u, &g) in unmatched.iter_mut().zip(&green[at]) {
                    *u -= g;
                }
            }
            for &at in group.positions() {
                for ((d, &g), u) in digit[at].iter_mut().zip(&green[at]).zip(&mut unmatched) {
                    let spend = g == 0 && *u > 0;
                    *d = if g == 1 { 2 } else { u8::from(spend) };
                    *u -= u8::from(spend);
                }
            }
        }
        out.fill(0);
        for d in &digit {
            for (o, &d) in out.iter_mut().zip(d) {
                *o = *o * 3 + u64::from(d);
            }
        }
    }
}

/// A guess letter that occurs once in the guess: when it is not green it is yellow exactly when
/// the word holds it anywhere, since every such occurrence is unmatched.
#[derive(Clone, Copy, Default)]
struct Once {
    /// Guess position.
    at: usize,
    /// Presence word holding the letter's bit.
    word: usize,
    /// The bit within it.
    shift: usize,
}

/// A guess letter that occurs more than once, with its positions in the guess.
#[derive(Clone, Copy)]
struct Group<const N: usize> {
    /// The letter's code.
    letter: u8,
    /// Its positions, left to right; the first `count` are used.
    positions: [usize; N],
    /// Number of positions.
    count: usize,
}

impl<const N: usize> Group<N> {
    /// The guess positions holding the letter, left to right.
    fn positions(&self) -> &[usize] {
        &self.positions[..self.count]
    }
}

/// A guess split into letters that occur once and letters that repeat, built once per guess.
pub struct Plan<const N: usize> {
    /// The guess's codes.
    guess: Code<N>,
    /// The once-only letters.
    once: [Once; N],
    /// How many of `once` are used.
    once_count: usize,
    /// The repeated letters.
    groups: [Group<N>; N],
    /// How many of `groups` are used.
    group_count: usize,
}

impl<const N: usize> Plan<N> {
    /// Splits `guess` by letter multiplicity.
    pub fn new(guess: &Code<N>) -> Self {
        let mut times = [0_u8; 256];
        for &g in guess {
            times[usize::from(g)] += 1;
        }
        let mut plan = Self {
            guess: *guess,
            once: [Once::default(); N],
            once_count: 0,
            groups: [Group {
                letter: 0,
                positions: [0; N],
                count: 0,
            }; N],
            group_count: 0,
        };
        for (at, &letter) in guess.iter().enumerate() {
            match times[usize::from(letter)] {
                0 => {}
                1 => {
                    let code = usize::from(letter);
                    plan.once[plan.once_count] = Once {
                        at,
                        word: code / PRESENCE_BITS,
                        shift: code % PRESENCE_BITS,
                    };
                    plan.once_count += 1;
                }
                _ => {
                    // the first occurrence collects them all; zero marks the letter as grouped
                    let group = &mut plan.groups[plan.group_count];
                    group.letter = letter;
                    for (to, &g) in guess.iter().enumerate().skip(at) {
                        if g == letter {
                            group.positions[group.count] = to;
                            group.count += 1;
                        }
                    }
                    plan.group_count += 1;
                    times[usize::from(letter)] = 0;
                }
            }
        }
        plan
    }

    /// The once-only letters.
    fn once(&self) -> &[Once] {
        &self.once[..self.once_count]
    }

    /// The repeated letters.
    fn groups(&self) -> &[Group<N>] {
        &self.groups[..self.group_count]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correctness::{LetterCounts, pattern_ordinal};
    use proptest::prelude::*;

    /// Every word of `words` against `guess`, kernel versus the scalar tally.
    fn kernel_matches_scalar<const N: usize>(
        guess: Code<N>,
        words: &[Code<N>],
        alphabet_len: usize,
    ) -> Result<(), TestCaseError> {
        let columns = Columns::gather(words.iter(), alphabet_len);
        let plan = Plan::new(&guess);
        let mut counts: LetterCounts = [0; 256];
        let mut out = [0; BLOCK];
        for k in 0..columns.blocks() {
            columns.ordinals(&plan, k, &mut out);
            for (b, &ordinal) in out.iter().enumerate().take(columns.block_len(k)) {
                let word = &words[k * BLOCK + b];
                prop_assert_eq!(
                    ordinal,
                    pattern_ordinal(word, &guess, &mut counts),
                    "word {:?} guess {:?}",
                    word,
                    guess
                );
            }
        }
        Ok(())
    }

    macro_rules! kernel_proptest {
        ($name:ident, $n:literal, $alphabet:literal) => {
            proptest! {
                #[test]
                fn $name(
                    guess in prop::array::uniform::<_, $n>(0u8..$alphabet),
                    words in prop::collection::vec(
                        prop::array::uniform::<_, $n>(0u8..$alphabet),
                        1..200,
                    ),
                ) {
                    const ALPHABET: u8 = $alphabet;
                    kernel_matches_scalar::<$n>(guess, &words, usize::from(ALPHABET))?;
                }
            }
        };
    }

    kernel_proptest!(kernel_n3_a3, 3, 3);
    kernel_proptest!(kernel_n4_a4, 4, 4);
    kernel_proptest!(kernel_n5_a3, 5, 3);
    kernel_proptest!(kernel_n5_a6, 5, 6);
    kernel_proptest!(kernel_n7_a4, 7, 4);
    kernel_proptest!(kernel_n9_a5, 9, 5);
    kernel_proptest!(kernel_n13_a4, 13, 4);
    kernel_proptest!(kernel_n17_a6, 17, 6);
    kernel_proptest!(kernel_n25_a4, 25, 4);
    kernel_proptest!(kernel_n25_a9, 25, 9);
    // letters spread over every presence word
    kernel_proptest!(kernel_n5_wide_alphabet, 5, 255);
    kernel_proptest!(kernel_n9_wide_alphabet, 9, 130);

    #[test]
    fn a_guess_letter_beyond_the_presence_words_is_absent_or_green() {
        let words: [Code<3>; 2] = [[0, 1, 2], [2, 1, 0]];
        // letter 200 needs presence word 3, which an Alphabet of three letters never allocates
        let guess: Code<3> = [200, 1, 0];
        let columns = Columns::gather(words.iter(), 3);
        let plan = Plan::new(&guess);
        let mut out = [0; BLOCK];
        columns.ordinals(&plan, 0, &mut out);
        let mut counts: LetterCounts = [0; 256];
        for (&ordinal, word) in out.iter().zip(&words) {
            assert_eq!(ordinal, pattern_ordinal(word, &guess, &mut counts));
        }
    }
}
