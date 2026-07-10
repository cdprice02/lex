# 07 — Pipeline simplification (drop tokio)

Depends on: nothing hard (any time after 00); coordinate with 02's
provenance stamping if done earlier
Unblocks: smaller dependency graph, faster builds, testable fetch path

## Why

`lex-data` pulls tokio (`features = ["full"]`), futures, async-compression,
and tokio-util to download files once per language — then wraps everything
back to sync (`blocking::DataDir`, one runtime per call) for the only
consumer. The code is confirmed cold-path. A sync rewrite deletes the entire
async surface.

## Requirements

- Replace async HTTP with **ureq** (rustls) — *not* `reqwest::blocking`,
  which still embeds a tokio runtime (ROADMAP decision) — and gzip via
  flate2's read adapters.
- Shard concurrency: keep ~4 concurrent downloads (`SHARD_CONCURRENCY` in
  lex-data/src/ngrams.rs) using `std::thread::scope` workers over a shared
  shard-index iterator or channel.
- Delete `lex-data/src/blocking.rs`; `DataDir` becomes plain sync. Update
  `lex-cli` (single consumer; breaking the API is fine pre-1.0).
- Remove tokio, tokio-util, futures, async-compression from Cargo.toml.
- **Retry** on shard fetch (TODO in ngrams.rs): 3 attempts, exponential
  backoff + jitter, on 5xx/connect/timeout. Hand-rolled loop; no middleware
  crate.
- **Filter during streaming**: `store::build_if_missing` loads the
  Wiktionary set *before* fetching — pass it into the fetch path and drop
  non-dictionary words and lengths outside `MIN_WORD_LENGTH..=MAX_WORD_LENGTH`
  at parse time. Removes the multi-GB accumulation peak and stops `populate`
  writing length buckets that can never be read.
- Testability: factor the line-parse loop to take `impl BufRead` so it can
  be tested with in-memory gzipped fixtures — no mock HTTP server.

## Open questions

- ureq vs. another minimal sync client (minreq, etc.) — ureq is the default
  proposal; confirm.
- Whether retry logic is unit-tested via an injected reader/transport or
  left covered by the fixture tests only.
- Progress reporting during long downloads (currently `log::debug!` per
  shard) — keep as-is or improve while in there.

## Out of scope

- Changing the on-disk cache format (task 03).
- Wiktionary parsing/validation semantics — output must be byte-identical
  for the same source data; verify by checksumming a rebuilt dict against
  the current pipeline's output for one small language.

## Done when

`cargo tree` shows no tokio/futures/async-compression in lex-data; a full
cold build of one small language (e.g. Italian, 2 shards) succeeds with
retry logic exercised at least in tests; dict/ngrams outputs are identical
to the async pipeline's for the same inputs.
