#!/bin/sh
set -e
name="${1:?usage: run.sh NAME DB_PROFILE WORKERS}"
profile="${2:?usage: run.sh NAME DB_PROFILE WORKERS}"
workers="${3:?usage: run.sh NAME DB_PROFILE WORKERS}"
namespaces="0|8|10|828|829|100"
folded_into_main="90 91 92 93 106 107 109 110 111 114 115 116 118 119"
wiki="http://localhost:8081/api.php?action=query&meta=siteinfo&format=json"

dir="$(cd "$(dirname "$0")" && pwd)"
cd "$dir"
export COMPOSE_FILE="$dir/docker-compose.yml"
export DB_PROFILE="$profile"
parts_dir="${PARTS_DIR:-$dir/parts}"
export PARTS_DIR="$parts_dir"
run_dir="$dir/runs/$name"
results="$dir/results.csv"

if [ ! -f "$parts_dir/manifest.tsv" ]; then
  echo "no split parts found in $parts_dir (run: cargo run --release -- split-dump --keep-every 10)" >&2
  exit 1
fi

min_free_gb="${MIN_DOCKER_FREE_GB:-8}"
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

echo "== $name: fresh install with db profile $profile"
docker compose down -v > /dev/null 2>&1 || true
rm -rf "$dir/config"
mkdir -p "$dir/config"
docker compose up -d --build > "$run_dir/up.log" 2>&1
until curl -sf "$wiki" > /dev/null; do sleep 3; done

query() {
  docker compose exec -T db mariadb -uwiki -pwiki wiki -N -e "$1"
}

query "select page_namespace, count(*) from page group by page_namespace order by page_namespace" | tr '\t' ' ' > "$run_dir/db-pages-before.txt"
bytes_before="$(query "select coalesce(sum(content_size), 0) from content")"

{
  echo "name=$name"
  echo "started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "db_profile=$profile"
  echo "workers=$workers"
  echo "namespaces=$namespaces"
  echo "parts_dir=$parts_dir"
  echo "docker=$(docker info --format 'cpus={{.NCPU}} memory_bytes={{.MemTotal}}')"
  echo "git_commit=$(git -C "$dir" rev-parse --short HEAD 2> /dev/null)"
  echo "db_settings=$(query "select concat('flush_log_at_trx_commit=', @@innodb_flush_log_at_trx_commit, ' doublewrite=', @@innodb_doublewrite, ' log_file_size=', @@innodb_log_file_size, ' buffer_pool_size=', @@innodb_buffer_pool_size)")"
  echo "profile_file:"
  cat "$dir/db-profiles/$profile.cnf"
} > "$run_dir/meta.txt"

parts="$(cd "$parts_dir" && ls part-*.xml)"
first_part="$(echo "$parts" | head -n 1)"
part_count="$(echo "$parts" | wc -l | tr -d ' ')"

query "select page_namespace, page_title, page_len from page order by page_namespace, page_title" | tr '\t' ' ' > "$run_dir/db-titles-before.txt"

docker compose exec -T mediawiki sh -c "sed '/^  <page>/,\$d' /parts/$first_part > /tmp/empty.xml && echo '</mediawiki>' >> /tmp/empty.xml && php maintenance/run.php importDump --no-updates /tmp/empty.xml" > "$run_dir/prepare.log" 2>&1

echo "== $name: sampling resources to $run_dir/resources.csv"
sh "$dir/../sample-resources.sh" "$run_dir/resources.csv" 2 &
sampler=$!

echo "== $name: importing $part_count parts with $workers workers"
start="$(docker compose exec -T mediawiki date +%s.%N)"
set +e
echo "$parts" | docker compose exec -T mediawiki xargs -P "$workers" -I{} sh -c \
  "php maintenance/run.php importDump --no-updates --report=1000 --namespaces '$namespaces' /parts/{} > /tmp/import-{}.log 2>&1"
status=$?
set -e
end="$(docker compose exec -T mediawiki date +%s.%N)"

kill "$sampler" 2> /dev/null || true
sampler=""

mkdir -p "$run_dir/import-logs"
for part in $parts; do
  docker compose exec -T mediawiki cat "/tmp/import-$part.log" > "$run_dir/import-logs/$part.log" 2> /dev/null || true
done

query "select page_namespace, count(*) from page group by page_namespace order by page_namespace" | tr '\t' ' ' > "$run_dir/db-pages-after.txt"
bytes_after="$(query "select coalesce(sum(content_size), 0) from content")"
imported="$(awk 'NR == FNR { before[$1] = $2; next } { sum += $2 - before[$1] } END { print sum + 0 }' "$run_dir/db-pages-before.txt" "$run_dir/db-pages-after.txt")"

awk -F'\t' -v namespaces="$namespaces" -v folded="$folded_into_main" '
  BEGIN {
    split(namespaces, wanted, "|"); for (i in wanted) keep[wanted[i]] = 1
    split(folded, fold, " "); for (i in fold) into_main[fold[i]] = 1
  }
  NR > 1 && $2 == "yes" {
    target = ($3 in into_main) ? 0 : $3
    if (target in keep) { pages[target] += $4; bytes += $5 }
  }
  END { for (n in pages) print n, pages[n]; print "bytes", bytes }
' "$parts_dir/manifest.tsv" | sort > "$run_dir/expected.txt"

{
  awk 'NR == FNR { before[$1] = $2; next } { delta = $2 - before[$1]; if (delta != 0) print $1, delta }' "$run_dir/db-pages-before.txt" "$run_dir/db-pages-after.txt"
  echo "bytes $((bytes_after - bytes_before))"
} | sort > "$run_dir/actual.txt"

if diff -q "$run_dir/expected.txt" "$run_dir/actual.txt" > /dev/null; then
  verify="ok"
elif [ -f "$dir/reference-actual.txt" ] && diff -q "$dir/reference-actual.txt" "$run_dir/actual.txt" > /dev/null; then
  verify="matches-reference"
else
  verify="MISMATCH"
fi

wall="$(awk -v s="$start" -v e="$end" 'BEGIN { printf "%.1f", e - s }')"
rate="$(awk -v n="$imported" -v w="$wall" 'BEGIN { if (w > 0) printf "%.2f", n / w; else print 0 }')"

if [ ! -f "$results" ]; then
  echo "name,db_profile,workers,parts,imported_pages,wall_seconds,pages_per_second,verify,exit_status" > "$results"
fi
echo "$name,$profile,$workers,$part_count,$imported,$wall,$rate,$verify,$status" >> "$results"
{
  echo "finished_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "imported_pages=$imported"
  echo "wall_seconds=$wall"
  echo "pages_per_second=$rate"
  echo "verify=$verify"
  echo "exit_status=$status"
} >> "$run_dir/meta.txt"

echo "== $name: $imported pages in ${wall}s ($rate pages/s), verify $verify, exit status $status"
