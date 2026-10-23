# CLAUDE.md — lex-data

## Purpose

Corpus acquisition and word type library. Owns all I/O: downloading Google Books Ngrams V3 shards and Wiktionary JSONL extracts, parsing, filtering, and caching to disk. Provides `Word<N>`, `WordSet<N>`, and `DataDir` to consumers.

## Module map

| Module | Responsibility |
|---|---|
| `data_dir.rs` | `DataDir` — async entry point; `load`, `dict_metadata`, `clear`, path helpers |
| `blocking.rs` | `blocking::DataDir` — sync wrapper; one tokio runtime per call |
| `store.rs` | `build_if_missing`, `read` (validates), `populate`, `filter_by_dict`, `clear`, `corpus_checksum` — all `pub(crate)` |
| `header.rs` | 64-byte cache header: encode, parse, `read_file` |
| `ngrams.rs` | `fetch(lang)` — streams V3 shards concurrently (`SHARD_CONCURRENCY = 4`) |
| `wiktionary.rs` | `fetch_dict`, `load_valid_words`, `load_metadata`, `DictMetadata` |
| `parse.rs` | V3 line parser; `normalize()` and `is_valid_word()` are `pub(crate)` for reuse in `wiktionary.rs` |
| `word.rs` | `Word<N>`, `WordSet<N>` (`from_pairs` keeps input order on frequency ties; `from_frequency_map` inherits HashMap order), `RawRecord<'a, N>` (zero-copy binary record view); exposes raw frequencies only (`word_frequencies`), the answer prior lives in `lex-core` |
| `language.rs` | `Language` enum with `lang_code()` and `iso_code()` |
| `error.rs` | `LexDataError` (incl. `LegacyCache`, `CorruptCache`, `CorruptRecord`) |
| `lib.rs` | `MIN`/`MAX_WORD_LENGTH` + `match_word_length!` dispatch macro (monomorphizes a fn over 3..=10; compile-time assert keeps the macro's literal bounds in lockstep with the consts) |

## Data layout

```
data/
  ngrams/{lang_code}/{N}.bin      — 64-byte header, then frequency-descending records: N×u32 (char LE) + u64 (freq LE) each
  dicts/{lang_code}.txt           — sorted Wiktionary word list (one per line)
  dicts/{lang_code}.meta.json     — DictMetadata: word_count, min/max word length, provenance (source_url, fetched_at, source_sha256 of the compressed download; absent in caches that predate stamping)
```

`lang_code` is the Google Books slug (e.g. `eng`, `fre`). See `Language::lang_code()`.

## Cache format

Header (little-endian, `header_len` bytes, 64 in v1): magic `LEXC`, version `u16`, `N` `u16`, `header_len` `u32`, record count `u64`, SHA-256 of the records region (32 bytes), zero padding. Records start at `header_len`, so a later version can grow the header with a version bump.

`store::read` checks magic/version/`N`, that the file length equals `header_len + count × record size`, the records checksum, and then every record (valid `char`s, frequency positive and non-increasing) before building the `WordSet`; all of it runs even when `limit` is set. Failures are typed `LexDataError`s naming the file (and record index) and are never auto-rebuilt. A file without the magic is a legacy cache: rejected, rebuilt with `lex clear`. See `docs/adr/0001-versioned-cache-header.md`. `WordSet` offsets are absolute, so `WordIter` assumes validity and is exact-size; `from_pairs` builds a headerless in-memory buffer.

## Build-if-missing pipeline

`store::build_if_missing::<N>` checks whether the ngrams binary exists. On a miss:

1. If `dicts/{lang}.txt` is absent → `wiktionary::fetch_dict` downloads and caches it
2. `wiktionary::load_valid_words` reads the word list into a `HashSet`
3. `ngrams::fetch(lang)` downloads all shards and returns a `HashMap<usize, HashMap<String, u64>>`
4. `store::filter_by_dict` removes any word not in the valid set; drops empty length buckets
5. `store::populate` writes all length buckets to disk

A single miss triggers a full language download — all length buckets are written at once, so subsequent requests for other lengths of the same language hit disk directly.

## Language codes

`Language::lang_code()` returns the Google Books URL slug and ngrams directory name (`eng`, `fre`, …).
`Language::iso_code()` returns the ISO 639-1 code used in KAIKKI Wiktionary URLs and JSONL records (`en`, `fr`, …). These are not systematically derivable from each other (e.g. German is `ger` / `de`, not `de` / `de`).

## Key invariants

- `Word<N>` chars must be NFC-normalized before construction; `try_from` counts Unicode scalars, not bytes
- `store::MIN_FREQUENCY = 1` — all Wiktionary-valid words are kept regardless of corpus frequency
- `parse::is_valid_word` rejects multi-word tokens, POS-tagged entries (`_NOUN`), and non-alphabetic strings
- `wiktionary.rs` takes `&Path` (not `&DataDir`) to avoid a circular import with `data_dir.rs`

## Provenance

KAIKKI extracts are rolling, so `fetch_dict` hashes the compressed bytes while streaming and stamps `DictMetadata`. `DataDir::ngrams_sha256` returns the corpus checksum from a cached `{N}.bin` header; that is the identity benchmark reports embed (the dict can change under a cache that was filtered against an older copy). `NGRAMS_SNAPSHOT` records the frozen ngrams release.
