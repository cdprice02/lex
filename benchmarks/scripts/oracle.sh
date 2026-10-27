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

tracks=$(
    for lang in $languages; do echo "$lang 5"; done
    echo "english 5 --hard-mode"
    echo "english 5 --profile wordle"
    echo "english 5 --profile wordle-all"
    if [ "$set_name" = full ]; then
        for lang in $languages; do
            for len in $(lengths); do
                [ "$len" -eq 5 ] || echo "$lang $len"
            done
        done
    fi
)

track_name() { # <lang> <len> [flags]: english-5, english-5-hard-mode
    printf '%s-%s%s' "$1" "$2" "$(printf '%s' "${3:-}" | tr -s ' -' '-')"
}

# A track whose flags the base binary lacks (a base from before `--profile`) cannot be compared
# and is skipped; once a base has the flag, the track is compared like any other.
base_supports() { # <track flags>
    case "$1" in
        *--profile*) "$base_dir/lex" simulate --help | grep -q -- --profile ;;
        *) true ;;
    esac
}

run_tracks() { # <lex binary> <output dir>
    mkdir -p "$2"
    echo "$tracks" | while read -r lang len flags; do
        out="$2/$(track_name "$lang" "$len" "$flags").json"
        [ -f "$out" ] && continue
        base_supports "$flags" || continue
        echo "oracle: $(track_name "$lang" "$len" "$flags") ($1)" >&2
        # A Profile fixes the language and length, and rejects both flags.
        case "$flags" in
            *--profile*) target="" ;;
            *) target="-l $len --lang $lang" ;;
        esac
        # $target and $flags are unquoted on purpose: they split into separate arguments
        # shellcheck disable=SC2086
        "$1" simulate $target -n 200 --seed 0 $flags \
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
    while read -r lang len flags; do
        track=$(track_name "$lang" "$len" "$flags")
        name="$track.json"
        if ! base_supports "$flags"; then
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
