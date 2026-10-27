#!/bin/sh
# Oracle: the working tree's `simulate` output must be byte-equal to the base
# commit's on every Oracle track (`games` and `summary`; the run timestamp
# differs by design). Exits non-zero on any difference.
#
# usage: benchmarks/scripts/oracle.sh <base-ref> [data-dir]
#
# Base reports are cached per commit under data/oracle/<sha>/, so later runs
# against the same base only rebuild and rerun the candidate.
set -eu

# Idle sleep freezes a run for as long as the machine sleeps; on macOS, hold
# the machine awake until this script exits.
if command -v caffeinate >/dev/null 2>&1; then
    caffeinate -i -w $$ &
fi

base_ref=${1:?usage: oracle.sh <base-ref> [data-dir]}
data_dir=$(cd "${2:-data}" && pwd)
repo=$(git rev-parse --show-toplevel)
base_sha=$(git rev-parse --verify "$base_ref^{commit}")
base_dir="$data_dir/oracle/$base_sha"
cand_dir=$(mktemp -d)
trap 'rm -rf "$cand_dir"' EXIT

tracks="english 3
english 4
english 5
english 5 --hard-mode
english 6
english 7
english 8
english 9
english 10
german 10
russian 6"

track_name() { # <lang> <len> [flags]: english-5, english-5-hard-mode
    printf '%s-%s%s' "$1" "$2" "$(printf '%s' "${3:-}" | tr -s ' -' '-')"
}

run_tracks() { # <lex binary> <output dir>
    mkdir -p "$2"
    echo "$tracks" | while read -r lang len flags; do
        out="$2/$(track_name "$lang" "$len" "$flags").json"
        [ -f "$out" ] && continue
        echo "oracle: $(track_name "$lang" "$len" "$flags") ($1)" >&2
        # $flags is unquoted on purpose: it splits into separate arguments
        # shellcheck disable=SC2086
        "$1" simulate -l "$len" --lang "$lang" -n 200 --seed 0 $flags \
            --data-dir "$data_dir" -o "$out.$$.tmp" </dev/null >/dev/null
        mv "$out.$$.tmp" "$out"
    done
}

# Concurrent runs against one base share its cache: the lock serializes the
# base build, and per-process temp names keep their report writes apart.
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
rmdir "$lock"
trap 'rm -rf "$cand_dir"' EXIT
run_tracks "$base_dir/lex" "$base_dir"

(cd "$repo" && cargo build --release --locked --bin lex)
run_tracks "$repo/target/release/lex" "$cand_dir"

status=0
echo "$tracks" | {
    while read -r lang len flags; do
        track=$(track_name "$lang" "$len" "$flags")
        name="$track.json"
        if [ "$(jq -S '{games, summary}' "$base_dir/$name")" = \
             "$(jq -S '{games, summary}' "$cand_dir/$name")" ]; then
            echo "same    $track"
        else
            echo "DIFFERS $track"
            status=1
        fi
    done
    exit $status
}
