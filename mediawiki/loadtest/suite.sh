#!/bin/sh
set -e
dir="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$dir/runs"

sh "$dir/run.sh" e4-bulksi-w16-a bulk-si 16
sh "$dir/run.sh" e4-bulksi-w16-b bulk-si 16

sh "$dir/report.sh"
