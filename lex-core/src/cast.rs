//! Numeric conversions between pool indices, feedback ordinals and the widths memory uses.
//!
//! Each is one inlined instruction, so the scoring loops that call them compile as they would
//! with a bare `as`.

/// A pool or candidate index as a position; `usize` is at least 32 bits on every supported target.
#[inline(always)]
#[must_use]
pub const fn index(i: u32) -> usize {
    i as usize
}

/// A position or length as a pool index; a pool holds at most `u32::MAX` words, and a word has
/// at most `MAX_WORD_LENGTH` letters.
#[inline(always)]
#[must_use]
pub const fn narrow(n: usize) -> u32 {
    n as u32
}

/// A length as a `u64`; `usize` is at most 64 bits on every supported target.
#[inline(always)]
#[must_use]
pub const fn widen(n: usize) -> u64 {
    n as u64
}

/// A feedback ordinal or count as an index into a buffer sized for it; the buffer is allocated
/// for `3^N` slots, which a target narrower than 64 bits could not hold at the lengths that need it.
#[inline(always)]
#[must_use]
pub const fn size(n: u64) -> usize {
    n as usize
}

/// A feedback ordinal as the `u32` the sorted counter keys on; it is only used at lengths whose
/// `3^N` fits.
#[inline(always)]
#[must_use]
pub const fn short(n: u64) -> u32 {
    n as u32
}

/// An Alphabet position as its one-byte code; `Pool::new` rejects an Alphabet over 255 letters.
#[inline(always)]
#[must_use]
pub const fn letter_code(i: usize) -> u8 {
    i as u8
}

/// A word frequency as a float for its logarithm; ngram counts stay far below 2^53.
#[inline]
#[must_use]
pub const fn lossy_f64(n: u64) -> f64 {
    n as f64
}

/// The `n`-th letter of `a..z`, wrapping, for fixtures.
#[cfg(any(test, feature = "bench"))]
#[must_use]
pub fn letter(n: usize) -> char {
    char::from(b"abcdefghijklmnopqrstuvwxyz"[n % 26])
}
