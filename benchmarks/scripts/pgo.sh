#!/bin/sh
# Build lex with profile-guided optimization (cargo-pgo): instrumented build, training runs,
# optimized build; prints the path of the optimized binary. Opt-in: a plain `cargo build --release`
# is unchanged.
# Usage (from the repo root): benchmarks/scripts/pgo.sh [--bolt] [--data-dir DIR] [-- extra simulate args]
#   --bolt      add a BOLT stage on top of the PGO build (Linux only)
#   --data-dir  the data directory every training run uses (the CLI ignores LEX_DATA_DIR)
#   GAMES=N     games per training run (default 200; the English and Russian runs match the Tracks)
# Training covers the Speed bar runs (English-5 and Russian-5 at -n 200, English-5 --pool hard at
# -n 50 for the hard-mode rollout paths) plus English-7 and Russian-9 at -n 20: LLVM treats code
# with no profile as cold, and a 5-letter-only profile slowed the Russian-9 book build by 10%, since
# each word length is its own monomorphized copy of the solver.
# Training runs play with the opening book like the measured runs, so a missing book is built first
# unless the extra args carry --no-book.
set -eu

BOLT=0 DATA=data
while [ $# -gt 0 ]; do
  case $1 in
    --bolt) BOLT=1 ;;
    --data-dir) DATA=$2; shift ;;
    --) shift; break ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
GAMES=${GAMES:-200}
HARD_GAMES=${HARD_GAMES:-50}
OTHER_GAMES=${OTHER_GAMES:-20}

if [ "$BOLT" = 1 ] && [ "$(uname -s)" != Linux ]; then
  echo "--bolt needs Linux: BOLT rewrites ELF binaries only" >&2
  exit 1
fi

TARGET=$(rustc -vV | sed -n 's/^host: //p')
BIN=target/$TARGET/release/lex

run_training() {
  bin=$1
  shift
  "$bin" simulate -l 5 --lang english -n "$GAMES" --data-dir "$DATA" --threads 1 "$@" >/dev/null
  "$bin" simulate -l 5 --lang russian -n "$GAMES" --data-dir "$DATA" --threads 1 "$@" >/dev/null
  "$bin" simulate -l 5 --lang english --pool hard -n "$HARD_GAMES" --data-dir "$DATA" --threads 1 "$@" >/dev/null
  "$bin" simulate -l 7 --lang english -n "$OTHER_GAMES" --data-dir "$DATA" --threads 1 "$@" >/dev/null
  "$bin" simulate -l 9 --lang russian -n "$OTHER_GAMES" --data-dir "$DATA" --threads 1 "$@" >/dev/null
}

cargo pgo build -- -p lex-cli
run_training "$BIN" "$@"

if [ "$BOLT" = 0 ]; then
  cargo pgo optimize build -- -p lex-cli
  echo "$BIN"
  exit 0
fi

cargo pgo bolt build --with-pgo -- -p lex-cli
run_training "$BIN-bolt-instrumented" "$@"
cargo pgo bolt optimize --with-pgo -- -p lex-cli
echo "$BIN-bolt-optimized"
