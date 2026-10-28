// The benches live in `#[cfg(feature = "bench")]` modules of the library, so they can reach
// crate-private items; this target links the library so their registrations are kept.
use lex_core as _;

fn main() {
    divan::main();
}
