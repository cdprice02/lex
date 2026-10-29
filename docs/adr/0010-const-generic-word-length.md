# Word length is a const generic up to the largest Supported length

Every word length from 3 to 21 is monomorphized: `Word<const N>` and the whole solver chain are instantiated once per length rather than switching long words to a runtime width. A runtime width costs 1.4x to 3x in the pattern loop, and a hybrid would duplicate the solver or push a word trait through every module; one const-generic codebase keeps every length on the fast path. Two choices follow from the wider range. Pattern ordinals are a single `u64` at every length (3^21 fits; a per-length width would need an impl per N and changes no float sum). Pattern mass is counted in a dense bucket array while 3^N x 8 bytes fits 16 MiB (N <= 13) and by stable sort-and-run above, a cut-over set by a memory bound rather than tuned per language. The patterns themselves are computed by letter count over the Corpus's Alphabet at every length, not only where it is fastest, so one technique serves all lengths. Lengths a language does not support (fewer than 200 words in its Corpus) are rejected at load, with an error naming the lengths it does support, not compiled out.

21 is the largest length because it is a Fibonacci breadth length (3, 8, 13, 21) and German and Russian both hold at least 200 words at every length up to it. 34 is out of reach (no language has 200 words there) and 27 is not a breadth length. A language's own limit is `Language::max_supported_length`, capped by `MAX_WORD_LENGTH`, and bounds the Track table.

## Considered Options

- Runtime width everywhere: rejected; it slows the headline tracks, which run at length 5.
- Const generics for 3 to 10 and a runtime-width path above: not built. It is a decision to be taken again, not an automatic fallback, if monomorphizing 3 to 21 more than doubles the clean release build time or binary size.

## Consequences

The monomorphized code is long; simplifying it without losing speed is tracked separately from this decision.
