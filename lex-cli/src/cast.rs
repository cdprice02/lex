//! Numeric conversions for report statistics.

/// A count as a float for averaging; counts stay far below 2^53.
#[must_use]
pub const fn count(n: usize) -> f64 {
    n as f64
}

/// A non-negative float rounded to a whole count, for bar widths and displayed tallies.
#[must_use]
pub fn rounded(x: f64) -> usize {
    x.round() as usize
}
