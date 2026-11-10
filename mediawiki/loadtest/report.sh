#!/bin/sh
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
baseline="${1:-$dir/baseline.csv}"
results="${2:-$dir/results.csv}"
full_pages=9570604

if [ ! -f "$baseline" ]; then
  sh "$dir/baseline.sh" > /dev/null
fi

if [ ! -f "$results" ]; then
  echo "name,db_profile,workers,parts,imported_pages,wall_seconds,pages_per_second,verify,exit_status" > "$results"
fi

awk -F, -v full="$full_pages" '
  FNR == NR {
    if (FNR > 1) {
      if ($1 == 1100000) early = $4
      final = $4
    }
    next
  }
  FNR == 1 {
    print "| Run | DB profile | Workers | Pages | Seconds | Pages/s | vs baseline (whole import) | vs baseline (first 1.1M pages) | Full dump (hours) | Verify |"
    print "|---|---|---:|---:|---:|---:|---:|---:|---:|---|"
    printf "| baseline (current database) | default | 1 | %d | %d | %.2f | 1.00x | 1.00x | %.2f | n/a |\n", full, full / final, final, full / final / 3600
    next
  }
  $9 != 0 {
    printf "| %s | %s | %s | %s | %s | %s | failed (exit %s) | failed | failed | %s |\n", $1, $2, $3, $5, $6, $7, $9, $8
  }
  $9 == 0 {
    printf "| %s | %s | %s | %s | %s | %s | %.2fx | %.2fx | %.2f | %s |\n", $1, $2, $3, $5, $6, $7, $7 / final, $7 / early, full / $7 / 3600, $8
  }
' "$baseline" "$results"
