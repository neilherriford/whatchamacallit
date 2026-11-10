#!/bin/sh
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
log="${1:-$dir/../config/import.log}"
out="${2:-$dir/baseline.csv}"

awk '
  /^[0-9]+ \([0-9.]+ pages\/sec [0-9.]+ revs\/sec\)/ {
    n = $1 + 0
    pages_rate = substr($2, 2) + 0
    revs_rate = $4 + 0
    if (pages_rate > 0) {
      elapsed = n / pages_rate
      last_n = n; last_pages_rate = pages_rate; last_revs_rate = revs_rate; last_elapsed = elapsed
      for (i = 1; i <= count; i++) {
        if (!done[i] && n >= milestone[i]) {
          done[i] = 1
          printf "%d,%.0f,%.2f,%.2f\n", n, elapsed, pages_rate, revs_rate
        }
      }
    }
  }
  BEGIN {
    count = split("100000 500000 1100000 2000000 5000000 10000000", milestone, " ")
    print "pages_counted,elapsed_seconds,pages_per_second,revs_per_second"
  }
  END {
    printf "%d,%.0f,%.2f,%.2f\n", last_n, last_elapsed, last_pages_rate, last_revs_rate
  }
' "$log" > "$out"

cat "$out"
