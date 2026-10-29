//! Numeric conversions between the widths the cache formats store and the widths memory uses.

/// A length or count as a stored `u64`; `usize` is at most 64 bits on every supported target.
#[expect(
    clippy::as_conversions,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[inline]
#[must_use]
pub const fn widen(n: usize) -> u64 {
    n as u64
}

/// A stored `u32` offset or length as an in-memory index; `usize` is at least 32 bits on every
/// supported target.
#[expect(
    clippy::as_conversions,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[inline]
#[must_use]
pub const fn index(n: u32) -> usize {
    n as usize
}

/// A stored `u64` count as an in-memory size; it truncates only on a target narrower than 64
/// bits, where a cache that large could not be mapped anyway.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[inline]
#[must_use]
pub const fn size(n: u64) -> usize {
    n as usize
}

/// A byte offset within a mapped Corpus as the `u32` the record index stores; the 1 GB Corpus
/// cap keeps every offset below `u32::MAX`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[inline]
#[must_use]
pub const fn offset(n: usize) -> u32 {
    n as u32
}
