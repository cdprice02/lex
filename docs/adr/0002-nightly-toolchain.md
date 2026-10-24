# Nightly toolchain, pinned

lex builds on a pinned nightly (`rust-toolchain.toml`, reproduced by the flake) for optimization headroom: `portable_simd`, `#[bench]`, `-Z` codegen flags, and the lints in use. The pin keeps builds reproducible; bumping it is a deliberate change verified against the benches.

## Consequences

The exit path, if stable is ever required: `variant_count` becomes a hardcoded const, `#[bench]` moves to criterion or divan, and `portable_simd` code needs a stable SIMD crate or scalar fallback.
