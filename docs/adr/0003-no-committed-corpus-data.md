# No committed corpus data, no git-lfs

Everything the solver reads is rebuilt from public sources into `data/`, never committed. Integrity comes from Cache header checksums and Provenance stamps rather than from version control. The same rule covers derived artifacts that are expensive to compute, such as precomputed pattern tables or a solved decision tree: they are built reproducibly into `data/`, keyed and checksummed by their inputs, and rebuilt rather than shipped.

## Considered Options

- git-lfs for corpora: rejected; it adds a hosting dependency for data anyone can regenerate, and the checksums already prove identity.
- Committing a solved decision tree: rejected; it would go stale whenever the Corpus, a Profile, or the Strategy changes, and the checksum key catches that automatically.
