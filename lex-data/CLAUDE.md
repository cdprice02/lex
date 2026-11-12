# CLAUDE.md: lex-data

Owns all I/O: downloading Ngrams shards and Wiktionary extracts, validating, caching to disk, and loading `Word<N>`, `WordSet<N>` and the stored records. The public surface is the re-exports in `lib.rs`; every module is private, so an item there is `pub` (`redundant_pub_crate` is on), and what a frontend may use is decided in `lib.rs`.

## Modules

| Module | Role |
|---|---|
| `data_dir` | `DataDir`: the entry point and every path under the data directory; carries the download worker count and the offline flag |
| `store` | Corpus files: build from totals or a download, `read` (validates), `ensure_supported`, `clear`, `corpus_checksum`; `build_all_via` takes the download as a parameter so tests need no network |
| `totals` | Ngrams totals (ADR 0013): `write` (all or nothing), `inspect`, `read_filtered` |
| `header` | the 64-byte file header shared by Corpus (`LEXC`) and totals (`LEXT`) files; `record_size(n)` |
| `word` | `Word<N>`, `WordSet<N>`, `RawRecord` (zero-copy record view) |
| `word_lookup` | read-only single-word lookups for a frontend; fetches and builds nothing |
| `dict_list`, `corpus_list`, `book_list`, `profile_list` | plain records of what is cached, read from disk only |
| `book_store` | Opening book files (ADR 0011): `BookKey`, stored nodes, read, write, clear |
| `profile`, `profile_store`, `list` | Profiles: the registry, fetch and verify, pure list parsing and validation |
| `ngrams`, `wiktionary`, `parse` | the two sources and the line parser (`normalize`, `is_valid_word`) |
| `http`, `retry`, `le` | one retried GET, the backoff, little-endian field reads |
| `language`, `error` | `Language` and `LexDataError` |
| `testing` | `test-support` feature only: seeds Corpus, Dictionary and totals files through the real writers |
| `lib` | length bounds and `match_word_length!` |

## Layout on disk

```
ngrams/{code}/{N}.bin     Corpus: header, then N x u32 chars + u64 frequency per record
totals/{code}/{N}.bin     Ngrams totals: same layout, magic LEXT, before the Dictionary filter
totals/{code}/meta.json   written last; a set without it is absent
dicts/{code}.txt          sorted word list; {code}.meta.json holds DictMetadata and provenance
profiles/{name}/          answers.txt, guess-only.txt, meta.json
books/{code}/{N}/{hash}.bin   Opening books
```

`{code}` is `Language::lang_code()` (the Google Books slug); `iso_code()` is the KAIKKI one. They are not derivable from each other.

## Invariants

- Header: little-endian, `header_len` bytes (64 in v1), records start at `header_len`. ADR 0001. A file without the magic is a legacy cache, rejected, never rebuilt on its own.
- `store::read` checks magic, version, `N`, file length, the records checksum, then every record; all of it runs when `limit` is set. Failures are typed `LexDataError`s naming the file.
- A Corpus is byte-reproducible: `encode_records` sorts by frequency descending, then by word. Files written before that rule tie-broke in `HashMap` order and are not rewritten.
- A set of totals is complete or absent. A file that disagrees with its `meta.json`, or a snapshot or shard count that is not this build's, means the set is downloaded again.
- `WordSet` offsets are absolute, so `WordIter` assumes validity. `from_pairs` keeps input order on frequency ties; `from_frequency_map` inherits `HashMap` order.
- `Word<N>` input must be NFC; `try_from` counts scalars, not bytes.
- Words are Wiktionary-valid at any frequency (`MIN_FREQUENCY = 1`). A length is Supported at `MIN_SUPPORTED_WORDS`. The Alphabet is at most `MAX_ALPHABET_SIZE`.
- Offline, an entry point that would reach the network returns `LexDataError::Offline` before any request; cached reads and computed work still run.
- `wiktionary` takes `&Path`, not `&DataDir`, to avoid an import cycle with `data_dir`.
- ureq is built without `gzip`; the `.gz` sources are decoded with `MultiGzDecoder`.
- A Profile's lists are re-hashed on every load against the compiled-in checksums; a mismatch is a hard error, never a refetch.

## Monomorphization

`match_word_length!` instantiates its callee for every length 3 to 21, so generic code is compiled 19 times. Keep a length-generic function a thin wrapper: do the work on a run-time `n` (see `store::open_checked`, `list::validate`, `profile_store::fetch_into`) and convert to `Word<N>` at the last step. Measure with `cargo llvm-lines` and `cargo bloat`.

## Fixtures

Tests seed caches through `lex_data::testing` (feature `test-support`, enabled by lex-cli's dev-dependency and lex-data's own). `tests/corpus_format.rs` holds the one hand-written byte layout of a Corpus file; every other fixture uses the writers, so only that test fails when writer and reader drift together.

## Commands

- Real Profile fetch: `LEX_CORPUS_DIR=<data> cargo test -p lex-data --test profile_fetch -- --ignored`
- Benches (divan, feature `bench`; `benches/lex-data.rs` is only `divan::main()`): `cargo bench -p lex-data --features bench`
