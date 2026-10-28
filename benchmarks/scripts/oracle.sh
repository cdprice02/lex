#!/bin/sh
# Oracle: the working tree's `simulate` output must be byte-equal to the base
# commit's on every track of the set (`games` and `summary`; the run timestamp
# differs by design). Exits non-zero on any difference.
#
# usage: benchmarks/scripts/oracle.sh [--full] <base-ref> [data-dir]
#
# The sets follow the Tiers of ADR 0007: the fast set (the default) is the
# headline Tier, and --full is every Tier.
#
# Base reports are cached per commit under data/oracle/<sha>/, so later runs
# against the same base only rebuild and rerun the candidate.
set -eu

# Idle sleep freezes a run for as long as the machine sleeps; on macOS, hold
# the machine awake until this script exits.
if command -v caffeinate >/dev/null 2>&1; then
    caffeinate -i -w $$ &
fi

usage='usage: oracle.sh [--full] <base-ref> [data-dir]'
set_name=fast
if [ "${1:-}" = --full ]; then
    set_name=full
    shift
fi
base_ref=${1:?$usage}
data_dir=$(cd "${2:-data}" && pwd)
repo=$(git rev-parse --show-toplevel)
base_sha=$(git rev-parse --verify "$base_ref^{commit}")
base_dir="$data_dir/oracle/$base_sha"
cand_dir=$(mktemp -d)
trap 'rm -rf "$cand_dir"' EXIT

languages="english french german spanish italian russian"

# The lengths the base commit supports; a length only the candidate supports
# has no base to compare against.
lengths() {
    src=$(git -C "$repo" show "$base_sha:lex-data/src/lib.rs")
    bound() { printf '%s\n' "$src" | sed -n "s/^pub const $1_WORD_LENGTH: usize = \([0-9]*\);/\1/p"; }
    seq "$(bound MIN)" "$(bound MAX)"
}

# One line per track: its id (the id `lex simulate --track` takes), then the `simulate` arguments
# that play it, in the vocabulary of the working tree. A Profile fixes the language and length,
# and rejects both flags.
tracks=$(
    for lang in $languages; do echo "$lang-5 -l 5 --lang $lang"; done
    echo "wordle --profile wordle"
    echo "wordle-all --profile wordle-all"
    echo "wordle-hard --profile wordle --pool hard"
    echo "wordle-ultrahard --profile wordle --pool ultrahard"
    if [ "$set_name" = full ]; then
        for lang in $languages; do
            for len in $(lengths); do
                [ "$len" -eq 5 ] || echo "$lang-$len -l $len --lang $lang"
            done
        done
    fi
)

# Whether the base binary has the flag `--pool`; a base from before it plays hard and ultrahard
# with `--hard` and `--ultrahard`.
base_has_pool() {
    "$base_dir/lex" simulate --help |
        grep -Eq -- '(^|[^[:alnum:]-])--pool([^[:alnum:]-]|$)'
}

# <binary> <args>: the args as that binary spells them.
args_for() {
    if [ "$1" = "$base_dir/lex" ] && ! base_has_pool; then
        printf '%s' "$2" | sed 's/--pool all//; s/--pool hard/--hard/; s/--pool ultrahard/--ultrahard/'
    else
        printf '%s' "$2"
    fi
}

# A track with a flag the base binary lacks (a base from before `--profile` or `--hard`) cannot be
# compared and is skipped; once a base has every flag, the track is compared like any other. The
# match is on the whole flag, so `--hard` is not satisfied by a base that only has `--hard-mode`.
base_supports() { # <track args>
    help=$("$base_dir/lex" simulate --help)
    for flag in $(args_for "$base_dir/lex" "$1"); do
        case "$flag" in
            --*) printf '%s\n' "$help" |
                grep -Eq -- "(^|[^[:alnum:]-])$flag([^[:alnum:]-]|\$)" || return 1 ;;
        esac
    done
}

run_tracks() { # <lex binary> <output dir>
    mkdir -p "$2"
    echo "$tracks" | while read -r id args; do
        out="$2/$id.json"
        [ -f "$out" ] && continue
        base_supports "$args" || continue
        echo "oracle: $id ($1)" >&2
        # $args splits into separate arguments on purpose
        # shellcheck disable=SC2046
        "$1" simulate $(args_for "$1" "$args") -n 200 --seed 0 \
            --data-dir "$data_dir" -o "$out.$$.tmp" </dev/null >/dev/null
        mv "$out.$$.tmp" "$out"
    done
}

# Concurrent runs against one base share its cache: the lock is held through
# the base build and the base reports, so a second run waits and reuses them,
# and per-process temp names keep report writes apart.
lock="$data_dir/oracle/build-$base_sha.lock"
mkdir -p "$data_dir/oracle"
until mkdir "$lock" 2>/dev/null; do sleep 5; done
trap 'rm -rf "$cand_dir"; rmdir "$lock" 2>/dev/null || true' EXIT
if [ ! -x "$base_dir/lex" ]; then
    tree="$data_dir/oracle/build-$base_sha"
    git -C "$repo" worktree add --detach "$tree" "$base_sha" >/dev/null
    (cd "$tree" && cargo build --release --locked --bin lex)
    mkdir -p "$base_dir"
    cp "$tree/target/release/lex" "$base_dir/lex"
    git -C "$repo" worktree remove --force "$tree"
fi
run_tracks "$base_dir/lex" "$base_dir"
rmdir "$lock"
trap 'rm -rf "$cand_dir"' EXIT

(cd "$repo" && cargo build --release --locked --bin lex)
run_tracks "$repo/target/release/lex" "$cand_dir"

status=0
echo "$tracks" | {
    while read -r track args; do
        name="$track.json"
        if ! base_supports "$args"; then
            echo "skipped $track (base has no such flag)"
        elif [ "$(jq -S '{games, summary}' "$base_dir/$name")" = \
             "$(jq -S '{games, summary}' "$cand_dir/$name")" ]; then
            echo "same    $track"
        else
            echo "DIFFERS $track"
            status=1
        fi
    done
    exit $status
}
