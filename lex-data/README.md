# lex-data

Corpus acquisition and word type library for the `lex` Wordle solver.

Downloads Google Books Ngrams V3 frequency data and Wiktionary word lists, filters the corpus to real dictionary words, and provides the `Word<N>` / `WordSet<N>` types used by the solver.

## Public API

```rust
use lex_data::blocking::DataDir;
use lex_data::{Language, WordSet};

// Load 5-letter English words (downloads on first call, cached after)
let dir = DataDir::new("data");
let words: WordSet<5> = dir.load(Language::English, None)?;

// Cap dictionary size
let top1000: WordSet<5> = dir.load(Language::English, Some(1000))?;

// Query word-length bounds from Wiktionary metadata
let meta = dir.dict_metadata(Language::French)?;
println!("{:?}", meta.word_length_range()); // e.g. 2..=31

// Invalidate cached files
dir.clear(Language::English, Some(5))?;  // one length
dir.clear(Language::English, None)?;     // whole language
```

## Key types

| Type | Description |
|---|---|
| `blocking::DataDir` | Sync entry point — wraps the async `DataDir` |
| `DataDir` | Async entry point; `load`, `dict_metadata`, `clear`, path helpers |
| `Language` | Enum: `English`, `French`, `German`, `Spanish`, `Italian`, `Russian` |
| `Word<N>` | Stack-allocated `[char; N]`, `Copy`, NFC-normalized |
| `WordSet<N>` | `HashMap<Word<N>, u64>` frequency map with probability helpers |
| `DictMetadata` | Word count + length range from the Wiktionary extract |
