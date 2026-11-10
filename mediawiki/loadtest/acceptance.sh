#!/bin/sh
set -e
name="${1:?usage: acceptance.sh NAME [split-dump options]}"
shift

namespaces="0|8|10|828|829|100"
folded_into_main="90 91 92 93 106 107 109 110 111 114 115 116 118 119"
wiki="http://localhost:8081/api.php?action=query&meta=siteinfo&format=json"

dir="$(cd "$(dirname "$0")" && pwd)"
cd "$dir"
export COMPOSE_FILE="$dir/docker-compose.yml"
export DB_PROFILE=bulk-si
export SKIP_IMPORT=
run_dir="$dir/runs/$name"
results="$dir/results.csv"

min_free_gb="${MIN_DOCKER_FREE_GB:-20}"
free_gb="$(docker run --rm alpine df -k / | awk 'NR == 2 { printf "%d", $4 / 1048576 }')"
if [ "$free_gb" -lt "$min_free_gb" ]; then
  echo "only ${free_gb} GB free in the Docker disk, need $min_free_gb (set MIN_DOCKER_FREE_GB to override)" >&2
  exit 1
fi

sampler=""
cleanup() {
  if [ -n "$sampler" ]; then
    kill "$sampler" 2> /dev/null || true
  fi
  docker compose stop > /dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

rm -rf "$run_dir"
mkdir -p "$run_dir"

echo "== $name: planning the import"
docker compose down -v > /dev/null 2>&1 || true
rm -rf "$dir/config"
mkdir -p "$dir/config"
cargo run --release --quiet --manifest-path "$dir/../../Cargo.toml" -- split-dump --ranges-only --out "$dir/config" "$@" > "$run_dir/plan.log"
cp "$dir/config/manifest.tsv" "$dir/config/ranges.tsv" "$run_dir/"

echo "== $name: starting the stack with the real entrypoint"
started="$(date +%s)"
docker compose up -d --build > "$run_dir/up.log" 2>&1

sh "$dir/../sample-resources.sh" "$run_dir/resources.csv" 2 &
sampler=$!

min_host_free_gb="${MIN_HOST_FREE_GB:-3}"
while [ ! -f "$dir/config/.imported" ] && [ ! -f "$dir/config/.import-failed" ]; do
  host_free_gb="$(df -k "$dir" | awk 'NR == 2 { printf "%d", $4 / 1048576 }')"
  if [ "$host_free_gb" -lt "$min_host_free_gb" ]; then
    echo "only ${host_free_gb} GB free on the host disk, stopping the run" >&2
    echo "stopped by acceptance.sh: host disk below $min_host_free_gb GB" > "$dir/config/.import-failed"
    docker compose stop > /dev/null 2>&1 || true
    break
  fi
  sleep 5
done
finished="$(date +%s)"

kill "$sampler" 2> /dev/null || true
sampler=""

import_started="$(cat "$dir/config/import-started")"
import_finished="$(cat "$dir/config/import-finished" 2> /dev/null || echo "$finished")"
mkdir -p "$run_dir/import-logs"
cp "$dir/config/import-logs/"* "$run_dir/import-logs/" 2> /dev/null || true

query() {
  docker compose exec -T db mariadb -uwiki -pwiki wiki -N -e "$1"
}

query "select page_namespace, count(*) from page group by page_namespace order by page_namespace" | tr '\t' ' ' | sort > "$run_dir/actual.txt"

awk -F'\t' -v namespaces="$namespaces" -v folded="$folded_into_main" '
  BEGIN {
    split(namespaces, wanted, "|"); for (i in wanted) keep[wanted[i]] = 1
    split(folded, fold, " "); for (i in fold) into_main[fold[i]] = 1
  }
  NR > 1 && $2 == "yes" {
    target = ($3 in into_main) ? 0 : $3
    if (target in keep) { pages[target] += $4 }
  }
  END { for (n in pages) print n, pages[n] }
' "$run_dir/manifest.tsv" > "$run_dir/expected-added.txt"

cat "$run_dir/expected-added.txt" "$dir/install-pages.txt" | awk '{ sum[$1] += $2 } END { for (n in sum) print n, sum[n] }' | sort > "$run_dir/expected.txt"

difference="$(awk '
  NR == FNR { expected[$1] = $2; seen[$1] = 1; next }
  { actual[$1] = $2; seen[$1] = 1 }
  END {
    for (n in seen) {
      d = actual[n] - expected[n]
      if (d != 0) print "namespace " n " differs by " d > "/dev/stderr"
      if (d < 0) d = -d
      total += d
    }
    print total + 0
  }
' "$run_dir/expected.txt" "$run_dir/actual.txt" 2> "$run_dir/differences.txt")"

imported="$(awk 'NR == FNR { before[$1] = $2; next } { sum += $2 - before[$1] } END { print sum + 0 }' "$dir/install-pages.txt" "$run_dir/actual.txt")"
part_count="$(( $(wc -l < "$run_dir/ranges.tsv") - 1 ))"
import_seconds="$((import_finished - import_started))"
cold_start_seconds="$((finished - started))"

if [ -f "$dir/config/.imported" ]; then
  status=0
  if [ -z "$*" ] && [ -f "$dir/reference-working-db.txt" ] && cmp -s "$dir/reference-working-db.txt" "$run_dir/actual.txt"; then
    verify="matches-working-db"
  elif [ "$difference" -le 4 ]; then
    verify="ok"
  else
    verify="MISMATCH"
  fi
else
  status=1
  verify="FAILED"
fi

rate="$(awk -v n="$imported" -v w="$import_seconds" 'BEGIN { if (w > 0) printf "%.2f", n / w; else print 0 }')"

if [ ! -f "$results" ]; then
  echo "name,db_profile,workers,parts,imported_pages,wall_seconds,pages_per_second,verify,exit_status" > "$results"
fi
echo "$name,$DB_PROFILE,$(docker compose exec -T mediawiki nproc | tr -d '\r'),$part_count,$imported,$import_seconds,$rate,$verify,$status" >> "$results"

{
  echo "name=$name"
  echo "kind=acceptance"
  echo "db_profile=$DB_PROFILE"
  echo "plan_options=$*"
  echo "docker=$(docker info --format 'cpus={{.NCPU}} memory_bytes={{.MemTotal}}')"
  echo "git_commit=$(git -C "$dir" rev-parse --short HEAD 2> /dev/null)"
  echo "cold_start_seconds=$cold_start_seconds"
  echo "import_seconds=$import_seconds"
  echo "imported_pages=$imported"
  echo "pages_per_second=$rate"
  echo "namespace_difference_total=$difference"
  echo "verify=$verify"
  echo "exit_status=$status"
} > "$run_dir/meta.txt"

echo "== $name: cold start ${cold_start_seconds}s, import ${import_seconds}s, $imported pages ($rate pages/s), verify $verify"
exit "$status"
