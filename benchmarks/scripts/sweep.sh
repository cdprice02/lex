#!/bin/sh
# Sweep answer-prior temperature (T) and win weight (lambda) on full-dictionary runs.
# Usage: benchmarks/scripts/sweep.sh LANG LENGTH OUTDIR [extra lex args, e.g. "--data-dir PATH"]
# Writes OUTDIR/T{t}-L{l}.json per cell, then prints a TSV of the summaries.
set -eu
LANG_=$1 LENGTH=$2 OUT=$3 EXTRA=${4:-}
LEX=${LEX:-target/release/lex}
export LANG_ LENGTH OUT EXTRA LEX
mkdir -p "$OUT"
for t in 1 2 5 10 20; do
  for l in 0 0.5 1 2 4; do
    echo "$t $l"
  done
done | xargs -P "${JOBS:-6}" -L 1 sh -c '
  "$LEX" simulate -l "$LENGTH" --lang "$LANG_" --temperature "$0" --win-weight "$1" \
    -o "$OUT/T$0-L$1.json" $EXTRA >/dev/null
'
printf 'T\tlambda\tuniform_mean\tprior_mean\tfailure_pct\tfirst_guess\n'
for t in 1 2 5 10 20; do
  for l in 0 0.5 1 2 4; do
    jq -r --arg t "$t" --arg l "$l" \
      '[$t, $l, (.summary.uniform.mean * 1000 | round / 1000), (.summary.prior.mean * 1000 | round / 1000), (.summary.uniform.failure_rate * 10000 | round / 100), .run.first_guess] | @tsv' \
      "$OUT/T$t-L$l.json"
  done
done
