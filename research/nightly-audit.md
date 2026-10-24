# Nightly and build-tooling audit (refresh, October 2026)

Resolves issue #11 (map #8). Refreshes the July 2026 audit against the current nightly.

## Method and evidence quality

- Pinned toolchain: `nightly-2026-05-28` (rustc 1.98.0-nightly 57d06900f, LLVM 22.1.6).
  A second local toolchain, `nightly-2026-08-09` (1.99.0-nightly 771916f90), was used as
  the "newer nightly" probe. Nothing newer than that is installed here, so anything dated
  after 2026-08-09 comes from tracking issues, not from a compiler.
- **Locally verified** items: a compiler told `#![feature(x)]` for a stabilized feature
  warns "stable since 1.NN.0", so stability was read off both toolchains directly. Small
  probe crates (generic_const_exprs, min_generic_const_args, portable_simd) were compiled
  on 2026-08-09.
- **From tracking issues** (fetched October 2026): portable_simd, trusted_len,
  iter_array_chunks, generic_const_exprs, portable-simd#364.
- **From general knowledge, not re-checked this session**: PGO and BOLT mechanics, CI
  runner CPU mix, build-std behaviour. Marked "unverified" below.
- Dev machine: Intel Core i5-7267U, `x86_64-apple-darwin`, `target-cpu=native` resolves
  to `skylake` (AVX2, FMA, BMI1/2, POPCNT; no AVX-512). The "Apple or Intel" question is
  answered: Intel.
- No benchmarks were run. Each "deciding measurement" below is a proposal.

## Summary table

| Item | State on 2026-08-09 nightly | Needs `unsafe` | Verdict |
|---|---|---|---|
| `slice::as_chunks` | stable since 1.88 | no | usable on stable; replaces `iter_array_chunks` need |
| `iter_array_chunks` (`Iterator::array_chunks`) | unstable, S-blocked (builds on `next_chunk`, bad ABI for large N) | no | drop from consideration |
| `array_windows` | stable since 1.94 | no | available, no known use |
| `select_unpredictable` | stable since 1.88 | no | candidate for branchless pass-2 logic |
| `core::hint::cold_path` | stable since 1.95 | no | candidate, low gain |
| `likely_unlikely` | still unstable | no | skip, `cold_path` covers it |
| `f64::algebraic_*` (`float_algebraic`) | stable since 1.98 (stable on 08-09, still gated on the 05-28 pin) | no | strongest new item for mass accumulation |
| `portable_simd` | unstable, blockers listed in portable-simd#364, no stabilization date | no | adopt for a bench-gated experiment only |
| `generic_const_exprs` | unstable, "implementation incomplete", compiles a basic `[f64; 3^N]` probe | no | not worth it for this project |
| `min_generic_const_args` | unstable prototype, rejects `[f64; p3(N)]` | no | cannot express 3^N yet |
| `TrustedLen` | unstable, no plan, trait is `unsafe` | **yes** | reject |
| `-Z build-std` | unstable cargo flag, works with `rust-src` already in the toolchain file | no | cheap experiment, small expected gain |
| PGO | stable rustc flags, needs `llvm-tools` component | no | best build-level candidate |
| BOLT | ELF only (x86-64, AArch64 ELF); no Mach-O | no | Linux-only, defer |
| `-C target-cpu=native` | fine locally | no | portability hazard, see CI section |
| `-Z threads=8`, `-Z share-generics` | still unstable flags, work | no | compile-time only; keep |
| `profile-rustflags` | still gated (`cargo -Z profile-rustflags`) | no | keep |

## Features currently in use (state check)

Source search (`feature(`) shows only: `test` (test cfg), `variant_count`,
`non_exhaustive_omitted_patterns_lint`, `must_not_suspend`. All four compile cleanly on both
nightlies with no "stable since" notice, so none has graduated. The audit's mention of
`profile-rustflags`, `-Z threads`, and `-Z share-generics` is cargo/rustc flags, not source
gates; all three are still accepted. `optimize_attribute` was removed in phase 06 and is not
revisited.

The exit costs in ADR 0002 still hold, and `portable_simd` is the only entry there that is
not currently exercised in code.

## Candidates in detail

### 1. Algebraic float ops for probability-mass accumulation (new, highest value)

`f64::algebraic_add` and siblings allow LLVM to reassociate, which is what blocks
vectorizing the `sum p*log2(p)` reduction and any bucket-sum loop. They are stable as of
1.98 (the 2026-08-09 toolchain emits a "stable since 1.98.0" warning for the gate), so they
need no `unsafe` and no feature flag after the bump.

- Expected gain: modest. `guess_entropy` sorts `touched` and sums in ascending ordinal
  order precisely so results are bit-identical. Reassociating that sum changes the last
  bits. Gain is real only if the final reduction is a measurable slice of the profile; per
  micro-opt.md the time is in `pattern_ordinal` and the touch/sort, so a few percent at
  best.
- Risk: **correctness contract**. `lex-core/CLAUDE.md` states the ascending order keeps
  guess sequences bit-identical, and the strict-greater tie-break on entropy means a 1-ulp
  shift can flip a guess. This would invalidate the oracle (saved `games` arrays) unless the
  oracle is rebaselined.
- Deciding measurement: sample a profile first (samply profile already exists). If the
  reduction is under 10% of `root_suggest_all_*` time, stop. Otherwise bench the
  algebraic variant and count how many of the four full-dictionary oracle runs change
  targets; any change means the contract must be renegotiated, not just measured.

### 2. `portable_simd` for `pattern_ordinal` (carry-over, experiment only)

Status: still unstable, no stabilization date. Blockers listed on portable-simd#364 are
the lane-count bound (`LaneCount<N>: SupportedLaneCount`), the mask element type (PRs
#483 and #493 are pending and will change the API), and swizzles (needs `adt_const_params`).
As of 2026-08-04 maintainers say two structural API changes are still to land first.
**Expect API churn**: any code written now may need rewriting at the next bump. A probe
using `Simd<u8, 8>` + `simd_eq` + `to_bitmask` compiles on 2026-08-09.

- Fit: a 5-letter word fits in `u8x8`; exact-match is one `simd_eq`. The misplaced pass
  (pass 2, with the claimed mask) is the hard part; it needs cross-lane compare, which on
  skylake is `pshufb`/broadcast-compare loops, not free. N up to 10 and the generic
  `char` element type (not `u8`) both complicate it. micro-opt.md already showed that
  shrinking `char` to `u8` alone gives nothing at scalar width, and that phase 09 "should
  measure under SIMD".
- Expected gain: unknown; plausible 1.5x to 3x on `pattern_ordinal` for N=5 on AVX2, if
  the load of word bytes is amortized across all guesses (transposed layout). Not
  verified.
- Risk: API churn, `u8` alphabet mapping needed (changes `Pool`), width-per-ISA
  differences, no `unsafe` needed if `Simd::from_array` / `from_slice` is used.
- Deciding measurement: a standalone `#[bench]` of `pattern_ordinal` vs a SIMD version over
  the 1000-word fixture (`root_suggest_all_1000_n5`), at N=5 and N=10. Adopt under the
  existing 10% bar and zero oracle drift. Result must be recorded for both skylake and a
  second CPU (see CI section), since lane widths differ.

### 3. PGO with the deterministic simulator as the workload

Stable rustc flags (`-C profile-generate`, `-C profile-use`). Needs `llvm-profdata`, which
the local toolchain does **not** have (`llvm-tools` component is not installed; the
toolchain file lists `rustfmt clippy rust-src rust-analyzer` only). `rustup component add
llvm-tools` or `cargo-pgo` would supply it. Works on macOS and Linux (unverified for the
Intel-mac combination; the mechanism is platform-neutral).

- Expected gain: typically 5 to 15% on branchy code, less on a tight loop with no
  unpredictable branches. This workload's hottest code is branch-light scoring, so lean to
  the low end. The decision-tree HashMap and sort paths may benefit more.
- Risk: profile must be regenerated whenever the code changes; profile data is
  toolchain-version specific; `lto = "fat"` interacts fine. Not reproducible across
  hosts unless the `.profdata` is committed or rebuilt in CI.
- Deciding measurement: instrumented build, run the English-5 default simulation (the
  deterministic oracle workload), rebuild with `profile-use`, compare wall time against
  39.5 s (phase 06 figure; re-measure for current main). Adopt at 10% or more. Verify
  oracle identical x4 (PGO should not change floating-point results, but check).

### 4. `-Z build-std`

`rust-src` is already in `rust-toolchain.toml`, so no setup cost. The config already
comments the option out. Benefit would be re-inlining std generic code with
`target-cpu=native` and LTO across std. For a program whose hot loop is entirely in this
workspace, the expected gain is small (under 3%, unverified). It also slows clean builds.

- Risk: CI cache keying, extra build time, needs an explicit `--target` triple in practice
  (cargo requirement; unverified for the exact current rule).
- Deciding measurement: same wall-time A/B as PGO. Cheap enough to run once; if under 3%,
  record as rejected.

### 5. `select_unpredictable`, `cold_path`, `as_chunks` (all stable)

- `select_unpredictable(cond, a, b)` (stable 1.88): a hint to emit a conditional move. In
  `pattern_ordinal`, pass 2's per-letter "found an unclaimed match" test is the branch
  candidate. The current code is already reported fast; the `[u8;N]` and bounds-check
  experiments moved less than 10%, so expect the same. Low risk, low gain.
- `core::hint::cold_path` (stable 1.95): mark the "touched bucket was 0.0, push to
  `touched`" or error paths. Only useful where LLVM guesses wrong; low gain.
- `slice::as_chunks` (stable 1.88): replaces the need for `iter_array_chunks`. Useful for
  chunking the candidate index list into SIMD-width batches if candidate 2 goes ahead. No
  `unsafe`. A compile-time-N variant, `as_chunks::<N>()`, returns `(&[[T;N]], &[T])`.
- Deciding measurement: each as a micro-change judged on `root_suggest_all_*` against the
  10% bar. Likely all fail the bar; adopt only if the code is simpler.

### 6. `generic_const_exprs` and `min_generic_const_args` for `[f64; 3^N]`

- `generic_const_exprs` (#76560): still open, label "implementation is incomplete", priority
  label dropped and T-types added in February 2026. No stabilization date. A probe
  (`struct Buf<const N: usize>([f64; p3(N)]) where [(); p3(N)]:;`) **compiles** on
  2026-08-09 with no ICE in this simple case. ICE-proneness is a claim from the July audit,
  not re-measured beyond this probe.
- `min_generic_const_args`: a prototype, being rewritten under the 2026 "Full Const
  Generics" project goal. The probe `[f64; p3(N)]` is **rejected** with "generic parameters
  may not be used in const operations" and a hint to add `generic_const_args` and a `type
  const` item. So arithmetic on `N` in array lengths is not available on the stable-bound
  track in the near term.
- Value: weak for this project. At N=10, `3^10 = 59,049` f64 is 472 KB, far too big for a
  stack array on a default 2 MB thread stack (worker threads) and wasteful for N=5 (243
  buckets). The existing sparse `touched` list was built for exactly the large-N problem.
  A stack buffer helps only N up to roughly 6 to 8, and the thread-local buffer already
  costs nothing per call.
- Verdict: do not adopt; if long-word support (#16) needs a different structure, prefer
  a heap-sized buffer or hashed buckets (a runtime choice) over const-generic arithmetic.

### 7. `TrustedLen` for `WordIter`

Open since 2016, `B-unstable`, no plan to stabilize. The trait is `unsafe` to implement.
**Requires `unsafe`; excluded by project policy.** `Iterator::size_hint` plus `collect`
into a pre-sized `Vec` gets nearly the same allocation behaviour without it.

### 8. `iter_array_chunks`

Open and `S-blocked` after a November 2025 libs-api review: it builds on unstable
`next_chunk`, whose return shape is poor for performance and "blows up stack space for
large N". The slice-level counterpart `as_chunks` is stable. Drop the item; the earlier
audit's guess ("stable `as_chunks` may already suffice") is confirmed.

### 9. BOLT

BOLT operates on **ELF** x86-64 and AArch64 binaries only (LLVM bolt README). macOS uses
Mach-O, so BOLT cannot run on the dev machine at all. It could only be a Linux CI or
release-build step, needs `llvm-bolt` and `merge-fdata`, requires non-stripped binaries
and relocations, and `cargo-pgo` marks its BOLT support experimental. Because the dev
loop is on macOS, any BOLT result is not reproducible on the machine where optimization
decisions are made. Defer unless there is a Linux-only release pipeline.

## `-C target-cpu=native` and CI portability

- Locally `native` is `skylake`: AVX2/FMA/BMI2. A binary built that way can fault with
  SIGILL on older CPUs, and macOS arm64 runners resolve it to `apple-*`. It is fine for
  a locally run solver and for benchmarks on the same host.
- Linux CI runners are a mix of CPU generations (unverified specifics; GitHub-hosted x86
  runners have varied between AMD EPYC and Intel, some with AVX-512). Consequences: (a)
  build artifacts or cached `target/` dirs reused across runners can crash or silently
  differ; (b) `native` makes timing comparisons across CI jobs noisy; (c) floating-point
  results depend on FMA contraction only if the compiler fuses `a*b+c`. Rust does not
  fuse by default, so `native` should not change bits, but this is worth one oracle check.
- Recommendation: keep `native` in `profile.release` for local work. For CI, build and
  run tests on one host (do not cache across hosts) or override with
  `RUSTFLAGS`/`--config` to a fixed level (`x86-64-v3` for AVX2, `x86-64-v2` baseline) for
  any published artifact. A fixed level also makes SIMD experiments portable: write for
  `x86-64-v3` (AVX2) and test the fallback.
- Caveat: `profile-rustflags` (unstable) overrides rather than merges with an environment
  `RUSTFLAGS`; check which source wins before relying on a CI override.
- Runtime dispatch (`is_x86_feature_detected!` plus `#[target_feature]`) avoids the issue,
  but calling a `#[target_feature]` function from non-matching code needs `unsafe`; out of
  scope for this project.

## Compile-time flags

- `-Z threads=8` and `-Z share-generics=yes` remain accepted and affect only build time.
  Under `lto = "fat"` the benefit of share-generics on release is small (unverified). Keep
  as is; not a runtime lever.
- `-Z profile-hint-mostly-unused` and `-Z trim-paths` exist in cargo; neither bears on
  runtime speed.

## Bumping the pin

Nothing in the four used features changed between `2026-05-28` and `2026-08-09`; both
compile the same gates. A bump to a newer nightly is justified mainly by one item:
`float_algebraic` going stable (1.98). Per ADR 0002, a bump must be verified against the
benches, so treat it as "bump, run `cargo bench -p lex-core` and the four oracle runs,
record numbers", not as a gain by itself.

## Recommended shortlist (feature, gain, risk, deciding measurement)

1. **PGO** on the English-5 default simulation. Gain 3 to 15% (guess). Risk: low, profile
   upkeep. Decide by wall time A/B at the 10% bar, oracle identical. Needs `llvm-tools`.
2. **`portable_simd` `pattern_ordinal`**. Gain unknown, possibly large at N=5. Risk: API
   churn, alphabet mapping, per-CPU width. Decide by an isolated `#[bench]` first, then the
   full run; second CPU for the check.
3. **`f64::algebraic_*`** in the entropy reduction. Gain small. Risk: breaks bit-identity
   contract. Decide only after a profile shows the reduction is over 10%.
4. **`-Z build-std`**: one-off A/B; expect under 3%.
5. **`select_unpredictable` / `cold_path` / `as_chunks`**: micro-changes, adopt only for
   simplicity; likely fail the 10% bar.

Rejected: `TrustedLen` (needs `unsafe`), `iter_array_chunks` (blocked), `generic_const_exprs`
and `min_generic_const_args` (low value, unfinished), BOLT (ELF only, not on the dev
machine).

## Effects on other tickets

- **#19 (speed-enabler ordering)**: put the toolchain-neutral, stable-safe items first (PGO
  setup, `llvm-tools` in the toolchain file, a profile of where time goes), then SIMD. Do
  the profile before any `algebraic_*` change, because that one carries the oracle risk.
  SIMD design should wait on the representation decision (alphabet `u8`), and be written
  against `x86-64-v3` so CI is portable.
- **#16 (long-word architecture)**: const-generic `[f64; 3^N]` buffers are not available
  on a stable track and would not fit the stack at N=10 anyway; plan the long-word path on
  heap or sparse buckets, which is what the code already does. `as_chunks` is stable and
  can batch candidates for SIMD at any N.
