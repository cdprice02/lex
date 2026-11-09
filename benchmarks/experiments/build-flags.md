# Build flags

**Verdict: pending.** Each variant below is measured alone against the plain `cargo build --release`
first; a variant is adopted only on its own Speed bar win, and the combination of the winners only if
it at least matches the best single winner.

Arbiter: the [Speed bar](../README.md#speed-bar) on end-to-end CPU time, base and candidate built
with the same profile and flags apart from the variant. Equivalence: the candidate's `simulate`
output is byte-identical to the plain build's on the two Speed bar Tracks and on English-7
(unprofiled for PGO), and the opening book is identical outside its digest. Tooling and recipes are
in [Build variants](../README.md#build-variants).

## Variants

| Variant | How it is built |
|---|---|
| codegen-units = 1 | `[profile.release] codegen-units = 1` |
| panic = "abort" | `[profile.release] panic = "abort"` |
| build-std | `-Z build-std=std` (or `std,panic_abort` with the panic variant) |
| PGO | `benchmarks/scripts/pgo.sh` |
| PGO + BOLT | `benchmarks/scripts/pgo.sh --bolt`, against PGO alone, on linden |
| Combined winners | every adopted variant above, one Speed bar against plain |

## Results

Machine, commit, rounds and spread to be recorded with each row.

| Variant | Machine | Track | Base s | Candidate s | Change | Output identical | Verdict |
|---|---|---|---|---|---|---|---|
| codegen-units = 1 | | | | | | | |
| panic = "abort" | | | | | | | |
| build-std | | | | | | | |
| PGO | | | | | | | |
| PGO + BOLT (vs PGO) | | | | | | | |
| Combined winners | | | | | | | |

## Book builds

The timed Russian-9 book build, for any variant that wins the Speed bar.

| Variant | Base s | Candidate s | Book identical outside its digest |
|---|---|---|---|
| | | | |
