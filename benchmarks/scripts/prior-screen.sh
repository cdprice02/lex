#!/bin/sh
# Screen Answer prior shapes against the default power prior on the length-5 Corpus Tracks and wordle.
# Usage: benchmarks/scripts/prior-screen.sh DATA_DIR OUTDIR
# Env: SHAPES (space-separated --prior-shape values), GAMES (targets per Track, default 2000),
#      JOBS (parallel runs), LEX (binary).
# Writes OUTDIR/{track}-{shape}.json per run (books are never written: --no-book), then pairs each
# shape against power with `lex compare` and prints the mean-guess deltas (negative is better).
set -eu
DATA=$1 OUT=$2
LEX=${LEX:-target/release/lex}
GAMES=${GAMES:-2000}
SHAPES=${SHAPES:-"sigmoid:0.1 sigmoid:0.25 sigmoid:0.5 floor:25 floor:50 floor:75"}
TRACKS="english french german spanish italian russian wordle"
export DATA OUT LEX GAMES
mkdir -p "$OUT"
for track in $TRACKS; do
  for shape in power $SHAPES; do
    echo "$track $shape"
  done
done | xargs -P "${JOBS:-4}" -L 1 sh -c '
  if [ "$0" = wordle ]; then game="--profile wordle"; else game="-l 5 --lang $0"; fi
  "$LEX" simulate $game --prior-shape "$1" -n "$GAMES" --seed 0 --no-book \
    --data-dir "$DATA" -o "$OUT/$0-$(echo "$1" | tr : -).json" >/dev/null
'
for shape in $SHAPES; do
  tag=$(echo "$shape" | tr : -)
  for track in $TRACKS; do
    printf '%s %s\n' "$shape" "$track"
    "$LEX" compare "$OUT/$track-power.json" "$OUT/$track-$tag.json" | grep '^mean guesses'
  done
done
