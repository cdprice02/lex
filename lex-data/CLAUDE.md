# CLAUDE.md — lex-data

## Purpose

Corpus acquisition and word type library. Owns all I/O: downloading Google Books Ngrams V3 shards and Wiktionary JSONL extracts, parsing, filtering, and caching to disk. Provides `Word<N>`, `WordSet<N>`, and `DataDir` to consumers.

## Module map

| Module | Responsibility |
|---|---|
| `data_dir.rs` | `DataDir`: sync entry point; `load`, `fetch_dict`, `build_corpus`, `dict_metadata` (fetches on a miss), the deletes `clear`, `clear_dict` and `clear_profile` (each returns the paths removed, and removes nothing and succeeds when the item is absent; `clear_dict` takes the Dictionary, that language's Corpus caches and their books), path helpers |
| `dict_list.rs` | `DictRecord` (plain record of a cached Dictionary: word list and metadata paths, size, word count read from the file, counts per Supported length counted in characters, count outside the range, optional `DictMetadata`, cached Corpus lengths) and `DataDir::{dicts, dict, cached_dict_metadata}`. Disk only: nothing is fetched, a language without a word list is absent, a word list without metadata still lists, metadata without Provenance has `None` fields. `metadata_mismatch()` gives the metadata count when it differs from the file. No rendering; the CLI and a later TUI picker render the records |
| `book_list.rs` | `BookRecord` (plain record of a stored opening book: language and length from its directory, path, size, id = file stem, `BookHeader` of format version, positions and `BookKey`, and a `BookStatus`) and `DataDir::{books, book_record, clear_stale_books}`; `BookFilter` narrows by language, length and Profile. Reads each file whole to verify its body checksum (the set is small), so a body-only corruption is `Corrupt` too (`BookRecord::verify_body` repeats the check for `book show`). Status: `Corrupt(reason)` for an unreadable header, a failed body checksum, a key whose hash is not its file name or a word length that disagrees with its directory; else `Stale(fields)` listing each of `solver_digest`, `corpus_sha256` and the Profile list checksums that is not the current one (stored and current value), else `Current`. The solver digest is a parameter (this crate cannot know it); the Corpus checksum is the cached `{N}.bin` header's, and an uncached Corpus never makes a book stale; Profile checksums are the compiled-in `ProfileSpec` ones, and a Profile name that is not built in is stale. A different Strategy is a different book, never stale. `clear_stale_books` removes exactly the stale and corrupt records under a filter |
| `corpus_list.rs` | `CorpusRecord` (plain record of a cached Corpus read from its header: path, size, format version, word count, full checksum, `CorpusStatus`, the filtering Dictionary's `fetched_at`; `supported()`) and `DataDir::{corpora, corpus, verify_corpus}`. Listing reads headers only (`Header::inspect` classifies `ok`, `legacy`, `old version`, `unreadable`); a file older than its Dictionary's `fetched_at` is `OlderThanDictionary`; `verify_corpus` hashes the records. Ordered by language then length; nothing is fetched |
| `profile_list.rs` | `ProfileRecord` / `ProfileListRecord` (a fetched Profile: fetch time from `meta.json`, per list the path, pinned source URLs, expected and actual checksum, word count; `verified()`, whether its Corpus is cached) and `DataDir::{profiles, fetched_profile}`. A Profile without `meta.json` is absent; nothing is fetched |
| `store.rs` | `build_if_missing` (one length missing builds all, via `build_all(dir, lang, progress)`, which `DataDir::build_corpus` exposes to `lex corpus build`), `ensure_supported` (a length under `MIN_SUPPORTED_WORDS` = 200 is `UnsupportedLength`, listing the language's Supported lengths from its cache headers; `DataDir::load` calls it), `read` (validates), `populate`, `clear`, `corpus_checksum`; all `pub` in a private module |
| `word_lookup.rs` | Read-only single-word lookups for a frontend: `DataDir::{dict_members, cached_corpus, profile_words}` (`None` when the Dictionary, Corpus or Profile is not cached; nothing is fetched, built or created; `cached_corpus` validates like `store::read` but leaves the Supported check to the caller) and `ProfileWords` (a fetched Profile's two lists as strings, not re-hashed). `normalize` and `is_valid_word` are re-exported at the crate root |
| `http.rs` | `agent()` (ureq, rustls, timeouts) and `get(agent, url, consume)`: one retried GET whose body is handed to `consume`; classifies errors transient or permanent |
| `retry.rs` | `with_retry(op, sleep)`: 3 attempts, 1 s then 2 s backoff plus jitter; `sleep` is injected so tests do not wait |
| `le.rs` | `array_at`: a fixed-size little-endian field read from a byte slice |
| `header.rs` | 64-byte cache header: encode, parse, `read_file` |
| `book_store.rs` | Opening book files at `books/<lang code>/<N>/<key hash>.bin` (ADR 0011): `BookKey` (typed Corpus, Profile, length and dictionary-length fields, the Strategy as ordered `(name, value)` strings rendered by `lex-core`, the solver digest; `fields()`, `differing_fields()`, `hash()`), `StoredBook`/`StoredNode` (the plain stored nodes, which this crate does not interpret), and `DataDir::{book_path, read_book, write_book, clear_books}` (`clear_books` returns the paths removed). Layout: `LEXB` magic, format version, word length, `header_len`, node count, body SHA-256, key length, then the postcard key and the postcard body. `read_book` returns the book, or a miss naming the fields on which the nearest sibling differs; a present file that fails its magic, version, checksum, parse or key check is `CorruptBook` (never a rebuild). `write_book` goes through `NamedTempFile::persist`. `clear` of a language and `clear_profile` also remove the matching books |
| `ngrams.rs` | `fetch(lang, valid, progress)` (`progress(done, total)` from the workers after each shard, first with 0): up to 4 scoped worker threads (`SHARD_CONCURRENCY`, fewer when the ambient rayon pool has fewer threads, via `worker_count`) pull shard indices from a shared counter and filter as they stream; `read_shard` takes any `BufRead` |
| `wiktionary.rs` | `fetch_dict`, `read_dict` (any `BufRead`), `load_valid_words`, `load_metadata`, `DictMetadata` |
| `parse.rs` | V3 line parser; `normalize()` and `is_valid_word()` are `pub` (in a private module) for reuse in `wiktionary.rs` |
| `word.rs` | `Word<N>`, `WordSet<N>` (`from_pairs` keeps input order on frequency ties; `from_frequency_map` inherits HashMap order), `RawRecord<'a, N>` (zero-copy binary record view); `alphabet()` (distinct letters; `ensure_alphabet_fits` rejects more than `MAX_ALPHABET_SIZE` = 255 with `AlphabetTooLarge`, called by `DataDir::load`), exposes raw frequencies only (`word_frequencies`), the answer prior lives in `lex-core` |
| `profile.rs` | `Profile` enum (`wordle`, `wordle-all`; strum `VariantArray`), `spec()` registry (language, length, `ListSpec`s of pinned `Source` URLs plus compiled-in checksums), `ProfileData`, `ProfileStats` |
| `profile_store.rs` | fetch (temp file then rename; `meta.json` last), re-hash on every load, build the pool and Answer list; `Get` is injected so tests skip the network |
| `list.rs` | pure list handling: `parse` (normalize, blank lines only at the end), `validate` (length, Alphabet, duplicates, file and line named), `canonicalize` (sorted, `\n` after each; its SHA-256 is the checksum) |
| `language.rs` | `Language` enum (strum `VariantArray`: `Language::VARIANTS`) with `lang_code()`, `iso_code()` and `max_supported_length()` (const, capped by `MAX_WORD_LENGTH`; bounds the Track table) |
| `error.rs` | `LexDataError` (incl. `LegacyCache`, `CorruptCache`, `CorruptRecord`, `CorruptBook`, `UnsupportedLength`, `UnsupportedWordLength`) |
| `lib.rs` | `MIN`/`MAX_WORD_LENGTH` + `match_word_length!` dispatch macro (monomorphizes a fn returning a `Result` over 3..=21, and any other length is `UnsupportedWordLength`; compile-time assert keeps the macro's literal bounds in lockstep with the consts) |

## Data layout

```
data/
  ngrams/{lang_code}/{N}.bin      — 64-byte header, then frequency-descending records: N×u32 (char LE) + u64 (freq LE) each
  dicts/{lang_code}.txt           — sorted Wiktionary word list (one per line)
  dicts/{lang_code}.meta.json     — DictMetadata: word_count, min/max word length, provenance (source_url, fetched_at, source_sha256 of the compressed download; absent in caches that predate stamping)
  profiles/{name}/answers.txt     — normalized Answer list (sorted, one word per line)
  profiles/{name}/guess-only.txt  — normalized guess-only words (empty for wordle-all)
  profiles/{name}/meta.json       — Provenance per list: sha256, word count, source URLs and checksums, fetched_at
```

`lang_code` is the Google Books slug (e.g. `eng`, `fre`). See `Language::lang_code()`.

## Cache format

Header (little-endian, `header_len` bytes, 64 in v1): magic `LEXC`, version `u16`, `N` `u16`, `header_len` `u32`, record count `u64`, SHA-256 of the records region (32 bytes), zero padding. Records start at `header_len`, so a later version can grow the header with a version bump.

`store::read` checks magic/version/`N`, that the file length equals `header_len + count × record size`, the records checksum, and then every record (valid `char`s, frequency positive and non-increasing) before building the `WordSet`; all of it runs even when `limit` is set. Failures are typed `LexDataError`s naming the file (and record index) and are never auto-rebuilt. A file without the magic is a legacy cache: rejected, rebuilt with `lex corpus clear`. See `docs/adr/0001-versioned-cache-header.md`. `WordSet` offsets are absolute, so `WordIter` assumes validity and is exact-size; `from_pairs` builds a headerless in-memory buffer.

## Build-if-missing pipeline

`store::build_if_missing::<N>` checks whether the ngrams binary exists. On a miss:

1. If `dicts/{lang}.txt` is absent, `wiktionary::fetch_dict` downloads and caches it
2. `wiktionary::load_valid_words` reads the word list into a `HashSet`
3. `ngrams::fetch(lang, &valid)` downloads all shards; each worker keeps only words in the set with a supported length (`MIN_WORD_LENGTH..=MAX_WORD_LENGTH`), and the merged totals below `MIN_FREQUENCY` are dropped
4. `store::populate` writes every length bucket to disk

Every download goes through `http::get`, which retries transient failures (connect, timeout, 5xx, 429, a dropped body) and fails at once on other 4xx. The sources are `.gz` files decoded with `MultiGzDecoder`; ureq is built without its `gzip` feature so it never decodes `Content-Encoding` itself.

A single miss triggers a full language download — all length buckets are written at once, so subsequent requests for other lengths of the same language hit disk directly.

## Language codes

`Language::lang_code()` returns the Google Books URL slug and ngrams directory name (`eng`, `fre`, …).
`Language::iso_code()` returns the ISO 639-1 code used in KAIKKI Wiktionary URLs and JSONL records (`en`, `fr`, …). These are not systematically derivable from each other (e.g. German is `ger` / `de`, not `de` / `de`).

## Key invariants

- `Word<N>` chars must be NFC-normalized before construction; `try_from` counts Unicode scalars, not bytes
- `store::MIN_FREQUENCY = 1`: all Wiktionary-valid words are kept regardless of corpus frequency
- A rebuilt `{N}.bin` is not byte-reproducible: `write_bucket` sorts by frequency with `sort_unstable`, so equal-frequency words land in `HashMap` order, which changes per process. The word set and frequency sequence are stable; the checksum is not
- `parse::is_valid_word` rejects multi-word tokens, POS-tagged entries (`_NOUN`), and non-alphabetic strings
- `wiktionary.rs` takes `&Path` (not `&DataDir`) to avoid a circular import with `data_dir.rs`

## Provenance

KAIKKI extracts are rolling, so `fetch_dict` hashes the compressed bytes while streaming and stamps `DictMetadata`. `DataDir::ngrams_sha256` returns the corpus checksum from a cached `{N}.bin` header; that is the identity benchmark reports embed; `DataDir::ngrams_count` reads the record count from the same header, so a Profile run can report the Corpus size without loading the Corpus a second time (the dict can change under a cache that was filtered against an older copy). `NGRAMS_SNAPSHOT` records the frozen ngrams release.

## Profiles

`DataDir::load_profile::<N>(Profile)` loads the Corpus of the Profile's language and length, fetches the lists if `meta.json` is absent, then re-hashes both stored files against the compiled-in checksums (a mismatch is a hard error, never a refetch). The Alphabet is derived from the Corpus. `pool` is the Profile's words only: Corpus words in Corpus order, then words the Corpus lacks, alphabetically, at the Unseen-word floor (the Corpus's minimum count). `wordle-all` stores its merged answers (both gists) and an empty `guess-only.txt`. The real-fetch test is `#[ignore]`: `LEX_CORPUS_DIR=<data> cargo test -p lex-data --test profile_fetch -- --ignored`.

## Micro-benchmarks

divan benches live in `#[cfg(feature = "bench")] mod benches` in `parse.rs` and `word.rs`; `benches/lex-data.rs` is only the `divan::main()` entry. Run `cargo bench -p lex-data --features bench`; a trailing argument filters by bench path (`-- parse_ngram_line`).
