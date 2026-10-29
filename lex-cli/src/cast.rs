//! Numeric conversions for report statistics.

/// A count as a float for averaging; counts stay far below 2^53.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[must_use]
pub const fn count(n: usize) -> f64 {
    n as f64
}

/// A non-negative float rounded to a whole count, for bar widths and displayed tallies.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the one audited conversion; the doc names the width invariant that makes it safe"
)]
#[must_use]
pub const fn rounded(x: f64) -> usize {
    x.round() as usize
}
