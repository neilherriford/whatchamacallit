#!/bin/sh
set -e
out="${1:?usage: sample-resources.sh OUTPUT.csv [INTERVAL_SECONDS]}"
interval="${2:-2}"

case "$out" in
  /*) ;;
  *) out="$PWD/$out" ;;
esac

cd "$(dirname "$0")"

info="$(docker info --format '{{.NCPU}},{{.MemTotal}}')"
echo "epoch_ms,service,cpu_usage_usec,memory_bytes,docker_ncpu,docker_mem_total_bytes" > "$out"

while true; do
  now="$(date +%s)000"
  for service in mediawiki db; do
    id="$(docker compose ps -q "$service")"
    usage="$(docker exec "$id" sh -c 'awk "/^usage_usec/ {print \$2}" /sys/fs/cgroup/cpu.stat')"
    memory="$(docker exec "$id" cat /sys/fs/cgroup/memory.current)"
    echo "$now,$service,$usage,$memory,$info" >> "$out"
  done
  sleep "$interval"
done
