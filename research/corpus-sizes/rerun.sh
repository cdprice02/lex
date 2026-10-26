#!/bin/sh
# Rerun the timings the first pass took under sleep or heavy load, recording CPU time so
# neither can inflate the figure. Usage: rerun.sh LEX DATA_DIR OUT_DIR
set -u
LEX=$1 D=$2 OUT=$3
mkdir -p "$OUT/rerun"
printf 'lang\tlength\treal\tuser\tsys\tstatus\tload_before\n' > "$OUT/rerun.tsv"
for row in english:8 english:9 english:10 french:9 french:10 german:6 german:7 german:8 german:9 german:10; do
  l=${row%:*} n=${row#*:}
  load=$(uptime | sed 's/.*averages*: //')
  /usr/bin/time -p timeout 1800 "$LEX" simulate -l "$n" --lang "$l" -n 500 --seed 0 --data-dir "$D" \
    -o "$OUT/rerun/$l-$n.json" >/dev/null 2>"$OUT/rerun/$l-$n.err"
  rc=$?
  t() { awk -v k="$1" '$1==k {print $2}' "$OUT/rerun/$l-$n.err" | tail -1; }
  case $rc in 0) st=ok;; 124) st=timeout;; *) st="exit=$rc";; esac
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$l" "$n" "$(t real)" "$(t user)" "$(t sys)" "$st" "$load" >> "$OUT/rerun.tsv"
done
echo done
