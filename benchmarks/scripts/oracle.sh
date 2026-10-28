#!/bin/sh
# Oracle: the working tree's `simulate` output must be byte-equal to the base
# commit's on every track of the set (`games` and `summary`; the run timestamp
# differs by design). Exits non-zero on any difference.
#
# usage: benchmarks/scripts/oracle.sh [--full] <base-ref> [data-dir]
#
# The sets come from the Suite manifest (benchmarks/suite.json) through the
# candidate's `lex bench --dry-run`: the fast set (the default) is the headline
# Tier plus the reported Tracks, and --full is every Track. A Track the base
# cannot run (a flag or a word length it lacks) is skipped.
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
tab=$(printf '\t')

# The word lengths the base commit supports; a length only the candidate
# supports has no base to compare against.
base_src=$(git -C "$repo" show "$base_sha:lex-data/src/lib.rs")
bound() { printf '%s\n' "$base_src" | sed -n "s/^pub const $1_WORD_LENGTH: usize = \([0-9]*\);/\1/p"; }
base_min=$(bound MIN)
base_max=$(bound MAX)

# The candidate's binary reads the candidate's manifest, even against an older base.
(cd "$repo" && cargo build --release --locked --bin lex)
cand_lex="$repo/target/release/lex"
listing=$("$cand_lex" bench --dry-run --manifest "$repo/benchmarks/suite.json")

# One "<id><TAB><simulate track arguments>" line per track of the set.
tracks=$(printf '%s\n' "$listing" | awk -F'\t' -v set="$set_name" \
    'set == "full" || $2 == "headline" || $3 == "yes" { print $1 "\t" $4 }')
[ -n "$tracks" ] || { echo "oracle: the manifest lists no tracks" >&2; exit 1; }

# A track with a flag the base binary lacks (a base from before `--profile` or `--hard`), or a
# word length outside the base's range, cannot be compared and is skipped; once a base has every
# flag, the track is compared like any other. The match is on the whole flag, so `--hard` is not
# satisfied by a base that only has `--hard-mode`.
base_supports() { # <track arguments>
    help=$("$base_dir/lex" simulate --help)
    prev=""
    for arg in $1; do
        case "$arg" in
            --*) printf '%s\n' "$help" |
                grep -Eq -- "(^|[^[:alnum:]-])$arg([^[:alnum:]-]|\$)" || return 1 ;;
        esac
        if [ "$prev" = -l ] && { [ "$arg" -lt "$base_min" ] || [ "$arg" -gt "$base_max" ]; }; then
            return 1
        fi
        prev=$arg
    done
}

run_tracks() { # <lex binary> <output dir>
    mkdir -p "$2"
    echo "$tracks" | while IFS=$tab read -r id args; do
        out="$2/$id.json"
        [ -f "$out" ] && continue
        base_supports "$args" || continue
        echo "oracle: $id ($1)" >&2
        # $args is unquoted on purpose: it splits into separate arguments
        # shellcheck disable=SC2086
        "$1" simulate $args -n 200 --seed 0 \
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

run_tracks "$cand_lex" "$cand_dir"

status=0
echo "$tracks" | {
    while IFS=$tab read -r track args; do
        name="$track.json"
        if ! base_supports "$args"; then
            echo "skipped $track (base lacks a flag or word length)"
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
