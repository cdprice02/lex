# 03 — Data integrity

Depends on: 02 (baselines verify these fixes are behavior-neutral)
Unblocks: 04 (artifacts need corpus identity), 06 (header carries the
alphabet table)

## Why

Two latent problems in `lex-data/src/word.rs` / `store.rs`:

- **Silent word/probability misalignment.** `WordIter::next` *skips* records
  whose stored scalar is not a valid `char`, but `scaled_probs` is computed
  over *all* offsets. `word_probs()` zips them — one bad record and every
  subsequent word pairs with the wrong probability, silently corrupting
  entropy scores. `WordIter::size_hint` also claims exactness it can't
  guarantee. We write these files ourselves so bad records "can't happen" —
  which is exactly why they must be a loud error, not a quiet skip.
- **No header.** The cache format has no magic, version, or record count;
  `read` silently ignores trailing bytes (`mmap.len() / SIZE`). Format
  evolution would parse old caches as garbage without complaint.

## Requirements

- Load-time validation: every record is checked once when the `WordSet` is
  constructed; an invalid record fails the load with a clear error naming the
  file and record index. After this, iteration can assume validity —
  `WordIter` becomes genuinely exact-size; fix `size_hint` and implement
  `ExactSizeIterator`.
- Cache file header containing at least: magic bytes, format version, `N`,
  record count, and the corpus checksum from task 02's provenance stamps.
  Reserve room (or plan a clean version bump) for the per-language alphabet
  table that task 06 adds.
- Migration story for existing caches (user has ~288 MB locally; rebuilding
  English means re-downloading 24 shards). Options: (a) legacy read path for
  one version, (b) a one-shot migrator, (c) document delete-and-rebuild.
  Propose one to the user before implementing.

## Open questions

- Exact header layout and magic value — propose.
- Whether `DictMetadata` (sidecar JSON) content should be duplicated in the
  binary header or referenced by checksum only.
- Checksum algorithm/scope (whole-file vs. records region).

## Out of scope

- The alphabet-indexed word *representation* itself (task 06) — only leave
  the header room for it.
- Nightly `TrustedLen` for `WordIter` — noted in ROADMAP as a candidate once
  exact-size is guaranteed; fine to do here if trivial, fine to skip.

## Done when

A corrupted record fails loudly with file + index; old-format caches are
handled per the agreed migration option; baselines from 02 reproduce exactly.
