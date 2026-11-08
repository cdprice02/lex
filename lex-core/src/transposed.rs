//! The Candidate set transposed for the entropy scan: one contiguous byte column per letter
//! position, and a letter-count byte per word and letter, scored against a guess a block of words at a
//! time. The kernel yields exactly [`pattern_ordinal`](crate::correctness::pattern_ordinal).

use std::simd::cmp::{SimdOrd, SimdPartialEq, SimdPartialOrd};
use std::simd::num::SimdUint;
use std::simd::{Mask, Select, Simd, SimdCast, SimdElement};

use crate::correctness::Code;

/// Words scored together. A multiple of every vector width the target may use, and small enough
/// that a block's working set stays in the L1 cache at the longest word length.
pub const BLOCK: usize = 64;

/// One block of a column of letter codes.
type ByteBlock = [u8; BLOCK];

/// Counts for a letter no word of the set can hold.
static NO_PRESENCE: ByteBlock = [0; BLOCK];

/// The words of a Candidate set as `N` letter columns plus the letters each word contains, padded
/// with words that match nothing to a whole number of [`BLOCK`]s.
pub struct Columns<const N: usize> {
    /// Number of real words.
    len: usize,
    /// `cols[i][k][b]` is letter `i` of word `k * BLOCK + b`; padding is `UNKNOWN_LETTER`.
    cols: [Vec<ByteBlock>; N],
    /// `counts[c * blocks + k][b]` is how many times word `k * BLOCK + b` holds the letter with
    /// code `c`: a plane per letter of the Alphabet, which is at most 255.
    counts: Vec<ByteBlock>,
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
        let mut counts = vec![[0_u8; BLOCK]; alphabet_len * blocks];
        for (at, word) in words.enumerate() {
            let (k, b) = (at / BLOCK, at % BLOCK);
            for (col, &code) in cols.iter_mut().zip(word) {
                col[k][b] = code;
                let letter = usize::from(code);
                if letter < alphabet_len {
                    counts[letter * blocks + k][b] += 1;
                }
            }
        }
        Self { len, cols, counts }
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
    pub fn ordinals<O: Ordinal>(&self, plan: &Plan<N>, k: usize, out: &mut [O; BLOCK]) {
        debug_assert!(
            N <= MAX_U32_LENGTH || size_of::<O>() == size_of::<u64>(),
            "an ordinal of {N} digits needs a u64"
        );
        for (half, out) in out.chunks_exact_mut(LANES).enumerate() {
            let at = half * LANES;
            let digits = self.digits(plan, k, at);
            combine(&digits, out);
        }
    }

    /// Block `k` of the count plane of the letter with code `letter`.
    fn plane(&self, letter: usize, k: usize) -> &ByteBlock {
        if letter < self.counts.len() / self.blocks().max(1) {
            &self.counts[letter * self.blocks() + k]
        } else {
            &NO_PRESENCE
        }
    }

    /// The feedback digit of each of the `LANES` words of block `k` from `at`, per guess
    /// position.
    fn digits(&self, plan: &Plan<N>, k: usize, at: usize) -> [Bytes; N] {
        let column = |i: usize| Bytes::from_slice(&self.cols[i][k][at..at + LANES]);
        let green: [Mask<i8, LANES>; N] =
            std::array::from_fn(|i| column(i).simd_eq(Bytes::splat(plan.guess[i])));
        let (zero, one, two) = (Bytes::splat(0), Bytes::splat(1), Bytes::splat(2));
        let mut digit = [zero; N];
        for &Once { at: pos, letter } in plan.once() {
            let held = Bytes::from_slice(&self.plane(letter, k)[at..at + LANES]);
            let yellow = held.simd_min(one);
            digit[pos] = green[pos].select(two, yellow);
        }
        for group in plan.groups() {
            // word letters equal to the guess letter that no green already matched
            let mut unmatched =
                Bytes::from_slice(&self.plane(group.letter.into(), k)[at..at + LANES]);
            for &pos in group.positions() {
                unmatched -= green[pos].select(one, zero);
            }
            for &pos in group.positions() {
                let spend = !green[pos] & unmatched.simd_gt(zero);
                digit[pos] = green[pos].select(two, spend.select(one, zero));
                unmatched -= spend.select(one, zero);
            }
        }
        digit
    }
}

/// The longest word whose feedback ordinal, 3^N - 1 at most, fits a `u32`.
pub const MAX_U32_LENGTH: usize = 20;

/// An integer a block's ordinals are written as: `u32` for words of up to [`MAX_U32_LENGTH`]
/// letters, which halves the bytes stored and widened, else `u64`.
pub trait Ordinal: SimdElement + SimdCast + Into<u64> + Default {}

impl Ordinal for u32 {}

impl Ordinal for u64 {}

/// Words scored together in one vector: an AVX2 register of bytes.
const LANES: usize = 32;

/// One vector of bytes, a feedback digit or letter code per word.
type Bytes = Simd<u8, LANES>;

/// Digits that fit a byte when folded base 3: `3^5 = 243`.
const CHUNK: usize = 5;

/// Folds the base-3 digits of each lane, position 0 most significant, into `out`. Digits fold in
/// chunks of [`CHUNK`] inside a byte, and the chunks in the narrowest integer that holds the
/// running value, so the wide multiplies are few.
fn combine<const N: usize, O: Ordinal>(digits: &[Bytes; N], out: &mut [O]) {
    let chunk = |c: usize| {
        let part = &digits[c * CHUNK..N.min((c + 1) * CHUNK)];
        part.iter()
            .fold(Bytes::splat(0), |acc, &d| acc + acc + acc + d)
    };
    let len = |c: usize| N.min((c + 1) * CHUNK) - c * CHUNK;
    let chunks = N.div_ceil(CHUNK);
    let mut narrow = chunk(0).cast::<u16>();
    let mut c = 1;
    // 3^10 fits a u16
    while c < chunks.min(2) {
        narrow = narrow * Simd::splat(pow3_u16(len(c))) + chunk(c).cast::<u16>();
        c += 1;
    }
    let mut mid = narrow.cast::<u32>();
    // 3^20 fits a u32
    while c < chunks.min(4) {
        mid = mid * Simd::splat(pow3_u32(len(c))) + chunk(c).cast::<u32>();
        c += 1;
    }
    if c == chunks {
        mid.cast::<O>().copy_to_slice(out);
        return;
    }
    let mut wide = mid.cast::<u64>();
    while c < chunks {
        wide = wide * Simd::splat(pow3_u64(len(c))) + chunk(c).cast::<u64>();
        c += 1;
    }
    wide.cast::<O>().copy_to_slice(out);
}

/// `3^n` as a `u16`, for `n` of at most 5 (a chunk's length).
const fn pow3_u16(n: usize) -> u16 {
    let mut p = 1_u16;
    let mut i = 0;
    while i < n {
        p *= 3;
        i += 1;
    }
    p
}

/// `3^n` as a `u32`, for `n` of at most 5.
const fn pow3_u32(n: usize) -> u32 {
    let mut p = 1_u32;
    let mut i = 0;
    while i < n {
        p *= 3;
        i += 1;
    }
    p
}

/// `3^n` as a `u64`, for `n` of at most 5.
const fn pow3_u64(n: usize) -> u64 {
    let mut p = 1_u64;
    let mut i = 0;
    while i < n {
        p *= 3;
        i += 1;
    }
    p
}

/// A guess letter that occurs once in the guess: when it is not green it is yellow exactly when
/// the word holds it anywhere, since every such occurrence is unmatched.
#[derive(Clone, Copy, Default)]
struct Once {
    /// Guess position.
    at: usize,
    /// The letter's code.
    letter: usize,
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
                    plan.once[plan.once_count] = Once {
                        at,
                        letter: usize::from(letter),
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

    /// Every word of `words` against `guess`, kernel versus the scalar tally, at ordinal width `O`.
    fn kernel_matches_scalar_as<const N: usize, O: Ordinal>(
        guess: Code<N>,
        words: &[Code<N>],
        alphabet_len: usize,
    ) -> Result<(), TestCaseError> {
        let columns = Columns::gather(words.iter(), alphabet_len);
        let plan = Plan::new(&guess);
        let mut counts: LetterCounts = [0; 256];
        let mut out = [O::default(); BLOCK];
        for k in 0..columns.blocks() {
            columns.ordinals(&plan, k, &mut out);
            for (b, &ordinal) in out.iter().enumerate().take(columns.block_len(k)) {
                let word = &words[k * BLOCK + b];
                prop_assert_eq!(
                    ordinal.into(),
                    pattern_ordinal(word, &guess, &mut counts),
                    "word {:?} guess {:?}",
                    word,
                    guess
                );
            }
        }
        Ok(())
    }

    /// The kernel at every ordinal width the length allows.
    fn kernel_matches_scalar<const N: usize>(
        guess: Code<N>,
        words: &[Code<N>],
        alphabet_len: usize,
    ) -> Result<(), TestCaseError> {
        kernel_matches_scalar_as::<N, u64>(guess, words, alphabet_len)?;
        if N <= MAX_U32_LENGTH {
            kernel_matches_scalar_as::<N, u32>(guess, words, alphabet_len)?;
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
        let mut out = [0_u64; BLOCK];
        columns.ordinals(&plan, 0, &mut out);
        let mut counts: LetterCounts = [0; 256];
        for (&ordinal, word) in out.iter().zip(&words) {
            assert_eq!(ordinal, pattern_ordinal(word, &guess, &mut counts));
        }
    }
}

#[cfg(feature = "bench")]
#[cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "bench functions are named for the divan report, not documented; clippy skips them in the test harness build"
    )
)]
mod benches {
    use divan::Bencher;
    use std::hint::black_box;

    use super::{BLOCK, Columns, Ordinal, Plan};
    use crate::candidates::synthetic_pool;

    /// The kernel alone at ordinal width `O`: 16 guesses over every block of 1000 candidates.
    fn ordinals_16<const N: usize, O: Ordinal>(b: Bencher) {
        let pool = synthetic_pool::<N>(1000);
        let words: Vec<_> = pool.indices().map(|i| *pool.code(i)).collect();
        let columns = Columns::gather(words.iter(), pool.alphabet_len());
        let plans: Vec<Plan<N>> = (0..16).map(|i| Plan::new(pool.code(i * 7))).collect();
        let mut out = [O::default(); BLOCK];
        b.bench_local(|| {
            for plan in &plans {
                for k in 0..columns.blocks() {
                    columns.ordinals(plan, k, &mut out);
                    black_box(&out);
                }
            }
        });
    }

    #[divan::bench(consts = [3, 5, 9, 15, 25])]
    fn ordinals_16_guesses_1000<const N: usize>(b: Bencher) {
        ordinals_16::<N, u64>(b);
    }

    #[divan::bench(consts = [3, 5, 9, 15])]
    fn ordinals_16_guesses_1000_u32<const N: usize>(b: Bencher) {
        ordinals_16::<N, u32>(b);
    }
}
