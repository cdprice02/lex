# Versioned cache header, checksum over records, no migration

Each Cache file starts with a self-describing header (magic, format version, word length, header length, record count, Corpus checksum). The checksum covers the word records only, so adding the header left every existing Baseline's checksum valid. Legacy caches are rejected with an instruction to rebuild rather than read through a second parser, and a corrupt Cache fails loudly instead of rebuilding silently, because a rebuild means re-downloading every ngrams shard and would hide the corruption.

## Considered Options

- Legacy read path for one version, or a one-shot migrator: rejected; the only caches were the author's, already rebuilt, and a second parser would outlive its purpose.
- Whole-file checksum: rejected; a file cannot hash its own header, and it would have invalidated the committed Baselines.
- Silent rebuild on corruption: rejected; it turns a data bug into a slow, unexplained download.
