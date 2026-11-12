# Download concurrency

**Verdict: `DEFAULT_DOWNLOAD_WORKERS` = 4.** From 4 workers up, a Corpus build runs at the speed
of the link; more workers add CPU time and peak memory and save no wall time.

Each worker streams one Ngrams shard at a time through gunzip and the parser, so it both waits on
the network and spends CPU. The question is where adding workers stops paying.

## Method

`main` at `dbc37da`, on linden (Ryzen 7 5800X, WSL2) under `linden-run.ps1`. Each run is
`lex corpus build --lang <lang> --download-threads <n> --no-progress` in a scratch data dir
seeded with only the Dictionary, so the run downloads every shard and writes the Ngrams totals
and every Corpus; the dir is deleted after the run. `/usr/bin/time -v` records wall, user and
system time and peak RSS. Settings ran once each in a mixed order (German 1, 8, 2, 4; English 24,
4, 12, 8), then 4 and its nearest rival 8 once more each. A lone `curl` of German shard 0
(774 MB) before and after each pass gives the link speed: 50.7 and 54.5 MB/s on the first pass,
54.4 and 52.7 MB/s on the repeat.

## Results

German, 8 shards (about 7 GB):

| workers | wall | user s | sys s | peak RSS |
|---|---|---|---|---|
| 1 | 3:36 | 136 | 12 | 1.93 GiB |
| 2 | 2:48 | 144 | 12 | 1.99 GiB |
| 4 | 2:37, 2:38 | 152, 149 | 17, 14 | 1.94, 2.00 GiB |
| 8 | 2:39, 2:38 | 157, 158 | 21, 22 | 2.24, 2.25 GiB |

English, 24 shards (about 10 GB):

| workers | wall | user s | sys s | peak RSS |
|---|---|---|---|---|
| 4 | 4:19, 4:18 | 231, 229 | 22, 24 | 2.64, 2.64 GiB |
| 8 | 4:17, 4:18 | 240, 236 | 31, 31 | 2.71, 2.68 GiB |
| 12 | 4:18 | 248 | 43 | 2.79 GiB |
| 24 | 4:16 | 257 | 51 | 2.98 GiB |

From 4 workers up, every wall time sits within 3 s of the others, and English moves about
39 MB/s, close to the single-shard `curl` rate: the link is the limit. One worker reaches about 32 MB/s,
below the link, and takes 37% longer than four. Above 4, user and system time and peak RSS
rise with the worker count while wall time stays flat.

## Choice

The curve flattens at 4 on both languages, so the default stays 4, capped at the language's shard
count. On a faster link the knee may sit higher; `--download-threads` (or an explicit
`--threads`) raises the count without a rebuild.
