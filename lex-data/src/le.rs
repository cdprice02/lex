//! Fixed-width reads from byte buffers.

/// The `L` bytes of `bytes` starting at `at`.
///
/// The caller has checked that the buffer reaches `at + L`; a shorter one is a bug at the call
/// site, not a data fault.
pub fn array_at<const L: usize>(bytes: &[u8], at: usize) -> [u8; L] {
    std::array::from_fn(|i| bytes[at + i])
}
