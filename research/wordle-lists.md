# Wordle list sourcing and published reference results

Research for issue #9 (map #8). Fetched 2026-10-23. Every number below carries its source; the sources section at the end gives full URLs. "Original lists" means the pre-NYT game: 2,315 answers, 12,972 allowed guesses (the guesses include the answers).

## Summary

- The original lists live at two pinned gist revisions (answers; guess-only words) and in Alex Selby's MIT-licensed repository as `wordlist_orig_hidden` and `wordlist_orig_all`. All three agree word for word.
- The ticket's "3.4201" is not the original-list optimum. It is SALET on the 2,309-answer list in force from 16 March to 29 August 2022 (7897 / 2309). On the original 2,315 answers the optimum is **3.4212** (7920 / 2315), reported by Selby and by Bertsimas and Paskov as 3.421.
- Every one of the 2,315 original answers is in the local English-5 Corpus. 1,063 of the 10,657 guess-only words are not (9.97%), so a Profile must carry its own Guess pool.
- No license is published for the lists. They are short factual word lists extracted from the game's JavaScript. Recommendation: fetch, verify SHA-256, and cache them at build or first use, with Provenance, the same way dictionaries are handled; do not vendor them into the repo.

## 1. List sources

### Original lists (2,315 answers, 12,972 guesses)

| Item | Value |
|---|---|
| Answers, pinned URL | https://gist.githubusercontent.com/cfreshman/a03ef2cba789d8cf00c08f767e0fad7b/raw/50838083dca5359b3cacf1ab677059b40c0e025e/wordle-answers-alphabetical.txt |
| Answers, gist revision | `c46f451920d5cf6326d550fb2d6abb1642717852` (committed 2025-04-26 per the gist API) |
| Answers, file SHA-256 | `5209b35f823f8b80f0404f863bd80df06d6a966c6eb1016d69f38badc6eed5d0` (13,889 bytes) |
| Guess-only, pinned URL | https://gist.githubusercontent.com/cfreshman/cdcdf777450c5b5301e439061d29694c/raw/cc443a0688b07ab5947afd3975160975547fc513/wordle-allowed-guesses.txt |
| Guess-only, file SHA-256 | `99be2e38dadf3e26952af7cb4d963f65b632d5de91aa99e5ce308e4dc9617b65` (63,941 bytes) |
| Selby answers | `wordlist_orig_hidden` in github.com/alex1770/wordle, SHA-256 `f40e47975cf360ff9febe7d49c8e285054d8fa3fb9644cea575922060951d6ee`, 2,315 lines |
| Selby answers + guesses | `wordlist_orig_all` (same word set as the gist union, different order), SHA-256 `fb66d9b47e0dad70e7c6c886cf76a6f453af3aa75003dcfa129c8326938e7c66`, 12,972 lines |

Facts verified by script on the downloaded files:

- Format: one lowercase ASCII word per line, `\n` delimited, all 5 letters, alphabetical. The gist answers file has no trailing newline; Selby's does. Normalizing the gist answers (sorted, `\n` after every word) gives exactly Selby's `wordlist_orig_hidden` SHA-256 above.
- The gist answers (2,315) and guess-only file (10,657) are disjoint. Their union is 12,972 words and has exactly the word set of `wordlist_orig_all` (Selby's file orders the words differently). So **every answer is also an allowed guess** in the game, but the guess-only gist does not repeat the answers; a Profile loader must union the two files.
- Answers are in alphabetical order, not the game's play order. The gist author says the original-order list is a separate gist, and that the NYT curates answers day by day since 2022-11-07, so no static NYT answer list exists.
- The gist raw URL without a revision hash tracks the latest revision (14 revisions, last active 2026-10-19). Pin the revision URLs above; at fetch time the unpinned and pinned files had identical hashes.
- Terms of use: the gist page states no license. The author says it is "not affiliated with the NYT". Selby's repo is MIT-licensed (GitHub API), but that covers his code; the lists are copied from the game. The NYT Terms of Service page could not be fetched (domain blocked), so the NYT's position on the lists is unverified.

### How the list versions changed (Selby, part 2, "Track of wordlist changes")

| Dates | Guess list | Answer list |
|---|---|---|
| before 15 Feb 2022 | 12,972 | 2,315 |
| 15 Feb to 15 Mar 2022 | 12,947 | 2,309 |
| 16 Mar to 29 Aug 2022 | 12,972 | 2,309 |
| from 30 Aug 2022 | 14,885 (part 2 text); 14,855 in part 3 and in the repo file | 2,309 |
| from Mar 2023 | same | 3,160, then 3,158 from July 2023 |

The 2,309 list is the 2,315 list minus six words: agora, fibre, lynch, pupal, slave, wench (checked against Selby's `wordlist_nyt20220830_hidden`, SHA-256 `c495f6cbc36475358a89e30ac16bd5c0c05073c8c7647bb6c35dd01f7bae6e32`). The current 3,158 list (`wordlist_hidden`, SHA-256 `3e443482b78dee0f6f8de4f74dbb67537db781524279b1d40ca3353c592b03ce`) adds 857 words to the 2,309 list and drops 8. Since 30 Aug 2022 the guess list is 14,855 words (`wordlist_all`, SHA-256 `ba86911aea83d038d53c0999fe6cbe310c8f59d2b79626c73bd0cd0773adc320`). Selby notes the 12,972 guess list was a revert on 16 March 2022, so "12972 / 2309" results apply to both the classic and March 2022 guess lists.

## 2. Overlap with the local English-5 Corpus

Method: parsed `data/ngrams/eng/5.bin` from the main checkout (LEXC header, 64 bytes; 28-byte records of five u32 code points plus a u64 count; 27,524 words, all lowercase) with a short throwaway script and compared word sets. The cache was built on this machine; the result depends on that Corpus build.

| List | Size | Missing from Corpus | Share |
|---|---|---|---|
| Original answers | 2,315 | **0** | 0% |
| Original guess-only | 10,657 | **1,063** | 9.97% |
| Original full guess list (union) | 12,972 | 1,063 | 8.19% |
| NYT answers, 2,309 (2022) | 2,309 | 0 | 0% |
| NYT answers, 3,158 (2023) | 3,158 | 1 | 0.03% |
| NYT guesses, 14,855 (2022 on) | 14,855 | 1,834 | 12.35% |

Sample missing guess-only words: abers, addio, adred, adsum, advew, aedes, afald, agila, agria, agros. They are Scrabble-style obscure words that the Wiktionary filter plausibly rejects. Conversely 15,615 Corpus words are not in the original 12,972 list, so the Corpus is a superset in size but not in content.

## 3. Reference results

Mean guesses are with a uniform prior over the answer list, equal to total guesses divided by answer count. "Failures" means not solved in 6 guesses. Lists are named by (guess count / answer count).

### 3.1 Proven optima, normal mode

| Result | Lists | Value | Source |
|---|---|---|---|
| SALET optimal, exhaustive proof, max 5 guesses, 0 failures | 12,972 / 2,315 | **3.4212** (total 7920) | Selby, Jan 2022 (Wayback copy); also repeated in Selby's README |
| Same, MIT group | 10,657 guesses / 2,315 | **3.42117** (SALET), 3.421 in text; max 5 | Bertsimas and Paskov preprint (Table 1, 3.1) and OR 2025 abstract |
| SALET optimal, post-NYT cuts | 12,972 / 2,309 | **3.4201** (total 7897); next best REAST 3.4214 | Selby part 2, 16 Mar 2022 section |
| Optimum after guess list grew | 14,885 / 2,309 | TARSE 3.4140 (7883); SALET 3.4188 (7894) | Selby part 2, 30 Aug 2022 section |
| Optimum on current answers | 14,855 / 3,158 | TARSE 3.5526 (11219); SALET 3.5576 (11235); max 6 | Selby part 3; Greenberg Table 1 |
| Worst opener QAJAQ | 12,972 / 2,315 | 4.1019 (9496) | Selby, Jan 2022 |

Top openers on the original lists (Selby Jan 2022, confirmed to five digits by Bertsimas and Paskov): SALET 3.4212, REAST 3.4225, CRATE and TRACE 3.4238, SLATE 3.4246, CRANE 3.4255. Bertsimas and Paskov also report that SALET reveals 1.683 coloured tiles on average versus 1.320 for AUDIO. The two groups' lists differ in description (Selby: 12,972 guesses including answers; Bertsimas and Paskov: 10,657 guesses) yet give identical totals, which suggests their 10,657 is the guess-only count with answers also allowed. The preprint text does not say so; treat this as unconfirmed.

### 3.2 Hard mode optimum

| Result | Lists | Value | Source |
|---|---|---|---|
| SALET optimal in hard mode (but needs up to 6 guesses) | 12,972 / 2,315 | **3.5084** (8122); next LEAST 3.5106 | Selby, Jan 2022 |
| Best with a 5-guess guarantee: PALET | 12,972 / 2,315 | 3.5448 (secondary, Gigazine's summary of Selby) | Gigazine, 29 Jan 2022 |
| SALET optimal hard mode | 12,972 / 2,309 | 3.5076 (8099) | Selby part 2, 16 Mar 2022 section |
| Hard mode, current answers | 14,855 / 3,158 | TARSE best per Selby part 3 strategy files; headline figures not extracted | Selby part 3 |

The Bertsimas and Paskov OR 2025 paper also solves hard mode, worst-case, and non-uniform variants (abstract), but its full text was not retrievable here and the 2022 preprint has no hard mode, so its hard mode figure is unverified. Selby's 3.5084 is the number to use.

Hard mode also has a stricter "superhard" variant (guesses must be possible answers). Greenberg reports an optimal superhard strategy starting SCAMP on 2,315 answers: mean 3.7162, max 5 (found by his own code after about three weeks).

### 3.3 Greedy and one-ply heuristics

| Heuristic | Guess rule | Lists | Mean | Max | Source |
|---|---|---|---|---|---|
| Max entropy (SOARE opener) | all guesses | 12,972 / 2,315 | 3.476 | 6 | Greenberg Table 1, citing Luebeck's solver page; the number is not on that page as fetched, so treat as unverified |
| Ad hoc heuristic (SOARE) | all guesses | 12,972 / 2,315 | 3.65 | 6 | Greenberg Table 1, citing Das's repo |
| negnumbins, "most parts" (TRACE) | guesses restricted to answers | 2,315 | 3.4600 | 6 | Greenberg Table 2 |
| negnumbins plus entropy tiebreak (TRACE) | answers only | 2,315 | 3.4553 | 6 | Greenberg Table 3 |
| negentropy, greedy max entropy (RAISE) | answers only | 2,315 | 3.4955 | 6 | Greenberg Table 2 |
| expbinsize (RAISE) | answers only | 2,315 | 3.5210 | 5 | Greenberg Table 2 |
| L-infinity (RAISE) | answers only | 2,315 | 3.5564 | 5 | Greenberg Table 2 |
| maxbinsize (ARISE) | answers only | 2,315 | 3.5844 | 5 | Greenberg Table 2 |
| negnumbins, entropy, on current answers (CARET/RAISE) | answers only | 3,158 | 3.6089 / 3.6431 | 7 | Greenberg Table 2 |

All of Greenberg's runs are 100% solved within 6 on the 2,315 list. Caveat for our benchmark: Table 2 restricts guesses to the answer list, so it compares to our hard mode style Strategy, not to the full Guess pool. Greenberg's own text says guessing only possible solutions loses little. His figures for a greedy entropy solver with the full guess list on 2,315 answers rest on the Luebeck 3.476 row only.

No source found reports a limited look-ahead (depth 2 or 3) result for a heuristic solver on these lists. Selby's exact search is the only look-ahead result, and it is full depth.

### 3.4 All 12,972 words as answers (our Wordle guess-set Profile)

| Result | Value | Source |
|---|---|---|
| Normal mode minimum worst case | 6 guesses (5 impossible, proved by Alex Peattie and confirmed by Selby and Poirrier) | Selby part 2; Poirrier |
| LANTS opener, 6-guess cap, optimal play | mean 4.07771 (total 52896 over 12,972); not proven the best opener | Selby part 2 |
| Hard mode minimum worst case | 7 guesses; only BLAWN, CHOMP, GUMBO, THUMB, THUMP, TUPIK, WHOMP can guarantee it | Selby part 2 |
| THUMP, hard mode, 7-guess cap | mean 4.52629 (58715), best opener within 7 guesses, proved | Selby part 2 |
| Greedy max entropy, TARES opener, 6 guesses | 99.67% solved (so not 0 failures) on the full dictionary Luebeck calls 12,927 words (likely a typo for 12,972) | Luebeck |
| Depth-5 tree, SABER opener, NYT-updated full list | mean 4.2361 (secondary, summarized from Poirrier's page) | Poirrier |

Selby's part 2 argues the 2,309 answer list is the natural problem because the answer list holds ordinary words and the other 10,638 are "Scrabble words". Selby part 3 notes that after March 2023 the game drew from the old 2,309 list on all but four of 120 days, so even the 3,158 list is partly academic.

## 4. Sourcing recommendation

1. Treat the original 2,315 / 12,972 pair as the canonical Wordle Profile for benchmarks: it is the only pair with a proven optimum (3.4212) and hard mode optimum (3.5084), and the lists never change. Name it by sizes, as Selby does ("2315/12972").
2. Fetch the two pinned gist URLs (answers; guess-only), verify the SHA-256 values above, and store them under `data/` with Provenance (URL, fetch time, checksum). Do not commit the lists to the repo, because no license covers them. If an offline-reproducible copy is wanted, Selby's `wordlist_orig_hidden` and `wordlist_orig_all` (MIT repository, hashes above) are an equally good mirror and carry the same content.
3. Define the checksum over the normalized form (sorted, lowercase, `\n` after every word), because the raw gist answers file lacks a trailing newline. The normalized answers hash is `f40e47975cf360ff9febe7d49c8e285054d8fa3fb9644cea575922060951d6ee`; the normalized full guess list (union, sorted) hash is `feacc549249b5dcb43e2799fbbcda14ebc33d55904fb15d043779104e60c1579`. Selby's `wordlist_orig_all` has the same word set but is not fully sorted, so its raw file hash differs; compare sorted word sets, not raw bytes.
4. Build the Guess pool as the union of the two files. Do not intersect with the Corpus: 1,063 guess-only words (and 1 of the 3,158 current answers) are absent from it.
5. Optional later profiles: NYT 2,309 (3.4201 reference) and NYT 3,158 / 14,855 (3.5526 reference), taken from Selby's repo files at a pinned commit.

## 5. Open items

- Bertsimas and Paskov OR 2025 hard mode and worst-case figures were not retrievable (publisher blocked, preprint lacks them).
- The Luebeck 3.476 and Poirrier 4.2361 figures are second-hand.
- The NYT Terms of Service for the lists was not read.

## Sources

- Selby, "The best strategies for Wordle" (19 Jan 2022), Wayback copy: https://web.archive.org/web/2024/http://sonorouschocolate.com/notes/index.php?title=The_best_strategies_for_Wordle
- Selby, part 2 (17 Mar 2022): https://web.archive.org/web/2024/https://sonorouschocolate.com/notes/index.php/The_best_strategies_for_Wordle,_part_2
- Selby, part 3 (25 Jul 2023): https://web.archive.org/web/2024/https://sonorouschocolate.com/notes/index.php/The_best_strategies_for_Wordle,_part_3_(July_2023)
- Selby repository (MIT): https://github.com/alex1770/wordle (README, `wordlist_*` files, fetched from the main branch on 2026-10-23)
- Bertsimas and Paskov, "An Exact and Interpretable Solution to Wordle" (2022 preprint): https://auction-upload-files.s3.amazonaws.com/Wordle_Paper_Final.pdf ; published as "An Exact Solution to Wordle", Operations Research 73(3):1384-1394 (2025), DOI 10.1287/opre.2022.0434 ; MIT Sloan summary: https://mitsloan.mit.edu/ideas-made-to-matter/how-algorithm-solves-wordle
- Greenberg, "Effective Wordle Heuristics", arXiv:2408.11730v2: https://arxiv.org/html/2408.11730v2
- Gigazine summary of Selby (29 Jan 2022): https://gigazine.net/gsc_news/en/20220129-the-best-strategies-for-wordle
- Luebeck, maximum entropy solver: https://jluebeck.github.io/posts/WordleSolver
- Poirrier, Wordle notes: https://www.poirrier.ca/notes/wordle/
- Gist lists by cfreshman: https://gist.github.com/cfreshman/a03ef2cba789d8cf00c08f767e0fad7b and https://gist.github.com/cfreshman/cdcdf777450c5b5301e439061d29694c
- Loyola recommended strategy page (confirms NYT list of 3,158 with 2,302 older words in 2026): https://rig.cs.luc.edu/~rig/wordle/recstrats.html
