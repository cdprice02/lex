# Nightly toolchain, pinned

lex builds on a pinned nightly (`rust-toolchain.toml`, reproduced by the flake) for optimization headroom: `portable_simd`, `-Z` codegen flags, and the lints in use. The pin keeps builds reproducible; bumping it is a deliberate change verified against the benches.

## Consequences

The nightly `test` feature (`#[bench]`) and `variant_count` are retired: benches use divan and `Correctness::COUNT` comes from strum's `EnumCount`. The pin stays for `non_exhaustive_omitted_patterns_lint`, `must_not_suspend`, the `-Z` codegen flags, and the planned `portable_simd` and `build-std`.

The exit path, if stable is ever required: the lints are dropped, and `portable_simd` code needs a stable SIMD crate or scalar fallback.
