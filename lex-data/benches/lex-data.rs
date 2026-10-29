//! The benches live in `#[cfg(feature = "bench")]` modules of the library, so they can reach
//! crate-private items; this target links the library so their registrations are kept.
#![feature(non_exhaustive_omitted_patterns_lint, must_not_suspend)]

use lex_data as _;

fn main() {
    divan::main();
}
