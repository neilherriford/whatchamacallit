#!/bin/sh
part="$1"
offset="$2"
length="$3"
cd /var/www/html

{
  head -c "$HEADER_LENGTH" "$DUMP_FILE"
  tail -c "+$((offset + 1))" "$DUMP_FILE" | head -c "$length"
  echo "</mediawiki>"
} | php maintenance/run.php importDump --no-updates --report=1000 --namespaces "$NAMESPACES" > "$LOGS/part-$part.log" 2>&1 \
  && touch "$LOGS/part-$part.done"
