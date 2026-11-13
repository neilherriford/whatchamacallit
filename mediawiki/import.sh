#!/bin/sh
set -u
cd /var/www/html

dump="${DUMP_FILE:?DUMP_FILE is not set}"
ranges=/config/ranges.tsv
logs=/config/import-logs
namespaces="0|8|10|828|100"
workers="${IMPORT_WORKERS:-$(nproc)}"
if [ "$workers" -gt 12 ]; then
  workers=12
fi

rm -rf /config/import-logs
rm -f /config/.import-failed /config/.imported
mkdir -p "$logs"

if [ ! -f "$ranges" ]; then
  echo "missing $ranges, run: make plan" > /config/.import-failed
  exit 1
fi

date +%s > /config/import-started

header_length="$(awk -F'\t' 'NR == 2 { print $2 }' "$ranges")"
total="$(( $(wc -l < "$ranges") - 1 ))"

{ head -c "$header_length" "$dump"; echo "</mediawiki>"; } \
  | php maintenance/run.php importDump --no-updates --namespaces "$namespaces" > "$logs/prepare.log" 2>&1
if [ $? -ne 0 ]; then
  echo "could not create the import user, see $logs/prepare.log" > /config/.import-failed
  exit 1
fi

export DUMP_FILE="$dump" HEADER_LENGTH="$header_length" NAMESPACES="$namespaces" LOGS="$logs"
tail -n +2 "$ranges" | awk -F'\t' '{ print $1, $2, $3 }' | xargs -P "$workers" -L 1 sh /import-part.sh
status=$?

date +%s > /config/import-finished
done_parts="$(ls "$logs"/part-*.done 2> /dev/null | wc -l | tr -d ' ')"

if [ "$status" -eq 0 ] && [ "$done_parts" -eq "$total" ]; then
  touch /config/.imported
else
  echo "$((total - done_parts)) of $total parts failed, see $logs" > /config/.import-failed
  exit 1
fi
