#!/bin/sh
# Fetch every language with no upper length cap, count Corpus words per length, then time
# sampled runs. Usage: run.sh LEX DATA_DIR OUT_DIR
set -u
LEX=$1 D=$2 OUT=$3
mkdir -p "$D" "$OUT/runs"
LANGS="english french german spanish italian russian"
code() { case $1 in english) echo eng;; french) echo fre;; german) echo ger;; spanish) echo spa;; italian) echo ita;; russian) echo rus;; esac; }

echo "phase A: fetch" >&2
printf 'lang\tfetch_seconds\n' > "$OUT/fetch.tsv"
for l in $LANGS; do
  if [ -f "$D/ngrams/$(code $l)/5.bin" ]; then echo "$l cached" >&2; continue; fi
  t0=$(date +%s)
  RUST_LOG=info "$LEX" simulate -l 5 --lang "$l" -n 1 --data-dir "$D" >"$OUT/fetch-$l.log" 2>&1
  printf '%s\t%s\texit=%s\n' "$l" "$(( $(date +%s) - t0 ))" "$?" >> "$OUT/fetch.tsv"
done

echo "phase B: counts" >&2
python3 - "$D" > "$OUT/counts.tsv" <<'PY'
import os, struct, sys
d = os.path.join(sys.argv[1], "ngrams")
print("lang\tlength\twords")
for lang in sorted(os.listdir(d)):
    for f in sorted(os.listdir(os.path.join(d, lang)), key=lambda x: int(x.split(".")[0])):
        b = open(os.path.join(d, lang, f), "rb").read(20)
        if b[:4] != b"LEXC":
            continue
        print(f"{lang}\t{f.split('.')[0]}\t{struct.unpack('<Q', b[12:20])[0]}")
PY

echo "phase C: sampled runs" >&2
printf 'lang\tlength\tseconds\tstatus\tload_before\n' > "$OUT/runs.tsv"
for l in $LANGS; do
  for n in 3 4 5 6 7 8 9 10; do
    words=$(awk -v c="$(code $l)" -v n="$n" '$1==c && $2==n {print $3}' "$OUT/counts.tsv")
    if [ -z "$words" ] || [ "$words" -lt 500 ]; then printf '%s\t%s\t\tskipped(<500)\t\n' "$l" "$n" >> "$OUT/runs.tsv"; continue; fi
    load=$(uptime | sed 's/.*averages*: //')
    t0=$(python3 -c 'import time; print(time.time())')
    timeout 900 "$LEX" simulate -l "$n" --lang "$l" -n 500 --seed 0 --data-dir "$D" -o "$OUT/runs/$l-$n.json" >/dev/null 2>"$OUT/runs/$l-$n.err"
    rc=$?
    secs=$(python3 -c "import time; print(round(time.time()-$t0, 1))")
    case $rc in 0) st=ok;; 124) st=timeout;; *) st="exit=$rc";; esac
    printf '%s\t%s\t%s\t%s\t%s\n' "$l" "$n" "$secs" "$st" "$load" >> "$OUT/runs.tsv"
  done
done
echo done >&2
