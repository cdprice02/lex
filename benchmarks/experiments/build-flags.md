# Build flags

**Verdict: no build-profile change.** `codegen-units = 1`, `panic = "abort"` and `-Z build-std`
are dropped: none moved either Track on either machine. PGO stays the opt-in
`benchmarks/scripts/pgo.sh`, not the default build or the Speed bar's: it wins 15 to 20% on the
Zen 3 machine and nothing on the Kaby Lake one, so a Speed bar on PGO binaries would measure the
profile as much as the change. BOLT on top of PGO adds 1 to 2% on Russian-5 only and stays behind
`--bolt`. With no second winner there was no combined run.

Arbiter: the [Speed bar](../README.md#speed-bar) on end-to-end wall time, base and candidate built
with the same profile and flags apart from the variant. Equivalence: the candidate's `simulate`
output (300 games) is byte-identical to the plain build's on the two Speed bar Tracks and on one
length outside PGO's training, and the opening book is identical outside its digest. Tooling and
recipes are in [Build variants](../README.md#build-variants).

Machines: **mac**, an i5-7267U (Kaby Lake, 2 cores, AVX2) on macOS; **linden**, a Ryzen 7 5800X
(Zen 3) under WSL2. Both built `-C target-cpu=native` at a6a5652. Times are the mean of 3 rounds,
the variants' order rotated each round, with the min to max spread in brackets.

## Variants

| Variant | How it is built |
|---|---|
| codegen-units = 1 | `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1` |
| panic = "abort" | `CARGO_PROFILE_RELEASE_PANIC=abort` |
| build-std | `-Z build-std=std --target $HOST`, `RUSTFLAGS="-C target-cpu=native"` |
| PGO | `benchmarks/scripts/pgo.sh` |
| PGO + BOLT | `benchmarks/scripts/pgo.sh --bolt`, against PGO alone, linden only (BOLT is ELF-only) |

## Results

Every variant's output was byte-identical to the plain build's.

| Variant | Machine | English-5 s | Russian-5 s | Verdict |
|---|---|---|---|---|
| plain | mac | 125.8 (123.2 to 127.6) | 278.3 (277.3 to 279.8) | base |
| codegen-units = 1 | mac | 127.5 (126.3 to 128.3) | 290.3 (281.6 to 300.4) | dropped |
| panic = "abort" | mac | 124.0 (122.4 to 126.7) | 278.8 (275.2 to 282.5) | dropped |
| build-std | mac | 125.5 (124.1 to 127.3) | 280.8 (277.9 to 283.9) | dropped |
| PGO, 5-letter training | mac | 125.5 (123.8 to 127.9) | 274.6 (274.4 to 274.7) | see below |
| plain | linden | 83.0 (82.8 to 83.1) | 187.9 (187.9 to 188.0) | base |
| codegen-units = 1 | linden | 83.9 (83.0 to 85.4) | 187.8 (187.7 to 188.1) | dropped |
| panic = "abort" | linden | 83.7 (83.3 to 84.3) | 189.3 (188.5 to 190.7) | dropped |
| build-std | linden | 83.1 (83.0 to 83.2) | 188.4 (187.1 to 191.0) | dropped |
| PGO, 5-letter training | linden | 72.3 (72.0 to 72.6) | 152.9 (151.6 to 154.7) | see below |
| PGO + BOLT, 5-letter training | linden | 71.4 (71.0 to 72.0) | 149.8 (149.7 to 149.9) | see below |

### Widened training

The 5-letter-only profile slowed the Russian-9 book build by 10% (below): each word length is its
own monomorphized copy of the solver, and LLVM treats an unprofiled copy as cold. `pgo.sh` now also
trains on English-7 and Russian-9 (`-n 20`); equivalence then checked English-6.

| Variant | Machine | English-5 s | Russian-5 s |
|---|---|---|---|
| plain | mac | 128.0 (123.7 to 132.1) | 279.9 (277.8 to 283.4) |
| PGO | mac | 128.3 (126.2 to 131.0) | 278.0 (274.8 to 282.7) |
| plain | linden | 82.7 (82.5 to 82.9) | 187.2 (186.8 to 187.6) |
| PGO | linden | 70.6 (69.4 to 72.7), -14.6% | 149.5 (148.7 to 150.0), -20.1% |
| PGO + BOLT | linden | 70.3 (70.0 to 70.6) | 148.0 (147.6 to 148.4) |

PGO's profile on mac carries the expected hot paths (the entropy columns, the transposed digits,
the Endgame floors), so the gap between the machines is in what the profile buys each core, not in
the training.

## Book builds

The timed Russian-9 book build on mac, one run each.

| Variant | Plain s | Candidate s | Book identical outside its digest |
|---|---|---|---|
| PGO, 5-letter training | 1,378 | 1,512 (+9.7%) | yes |
| PGO, widened training | 1,378 | 1,400 (+1.6%) | yes |
