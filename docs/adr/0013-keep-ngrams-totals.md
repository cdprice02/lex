# Keep Ngrams totals, not the shards

A download of a language's ngrams shards (35 GB over six languages, 13.4 GB for English alone) writes its Corpus and discards the source, so a Corpus can only be rebuilt by downloading everything again. The build keeps instead the **Ngrams totals**: every word that passes `is_valid_word` at a length of 3 to 21 with its count summed over all years, before the Dictionary filter. They are small because most of a shard is per-year rows, POS-tagged tokens and multi-word tokens the build discards. A Corpus is then the totals filtered by a Dictionary, and a missing Corpus, `corpus build --force`, and a rebuild after `dict fetch --force` all run offline.

The totals use the Corpus cache format under the magic `LEXT` (ADR 0001's header, the same records), one file per length at `data/totals/{code}/{N}.bin`, so the Corpus reader and writer carry over and each reader rejects the other's files with a typed error. A set is written into a temporary directory and renamed into place with its `meta.json` (snapshot, shard count, per-file checksums) written last; a set without it, or whose files disagree with it, is absent, downloaded again with a warning and never used. One download writes the totals and the Corpus from the same in-memory totals, and a test holds the two paths to the same bytes. To make that comparison exact, `write_bucket` breaks frequency ties by word, so a Corpus rebuild is byte-identical; Corpus files already on disk are left as they are, and a build from totals writes only the lengths not cached, so the Track snapshots that pin `corpus_sha256` keep matching.

`corpus clear` keeps the totals (they do not depend on the Dictionary, and `dict clear` leaves them too); `corpus clear --all-data` removes them. Existing installs get totals with a language's next download; nothing downloads in the background. The budget is a peak of 4 GB of memory and 2 GB of totals on the largest language, English; a build over either stops for a per-length spill to disk or compression, neither of which is done until measured.

## Considered Options

- Keeping the raw shards: rejected; 35 GB against a data directory of a few hundred MB, for a source whose checkability (re-deriving any Corpus) the totals already give at the Dictionary-filter step.
- Keeping the Dictionary-filtered counts only: rejected; a changed Dictionary (`dict fetch --force`) would need the download again, which is the cost this removes.
- A separate format for the totals: rejected; the Corpus format, reader and checksum already fit, and the magic separates the kinds.
- Compressing the totals: rejected until measured; the records are already a fraction of the source.
- Leaving frequency ties in hash order: rejected; it makes every rebuild a new Corpus checksum, which the Track snapshots pin, so an offline rebuild could not be checked against the download it replaces.
