#!/bin/sh
# Capacity sweep, pdfrs vs pdfjs, everything in Docker on the same VM (k6 included):
# for each config, hold VUS concurrent users for 15 s, print req/s, p95 and peak memory of the server container.
# Usage: PDFJS_DIR=../pdfjs sh bench/sweep.sh [config...]
#   configs: rs (all cores), rs1 (1 core), js1 (Node x 1), jsN (Node x cores), js1c (Node x 1 on 1 core)
cd "$(dirname "$0")/.." || exit 1
PDFJS_DIR=$(cd "${PDFJS_DIR:-../pdfjs}" && pwd)
NET=pdfbench
DURATION=${DURATION:-15s}
CORES=$(docker info --format '{{.NCPU}}')
docker network create $NET >/dev/null 2>&1
docker build -q -t pdfrs . >/dev/null || exit 1
# pdfjs's dependencies, installed once for Linux in a volume (the host's node_modules is untouched)
docker run --rm -v "$PDFJS_DIR":/app -v pdfjs-node-modules:/app/node_modules -w /app node:26-alpine \
  sh -c '[ -d node_modules/react ] || npm ci --omit=dev --no-audit --no-fund >/dev/null'

start() { # name, extra docker args...
  name=$1; shift
  docker rm -f srv >/dev/null 2>&1
  case $name in
    rs)   docker run -d --name srv --network $NET -v "$PWD/demo":/images:ro -e IMAGES_DIR=/images "$@" pdfrs ;;
    rs1)  docker run -d --name srv --network $NET -v "$PWD/demo":/images:ro -e IMAGES_DIR=/images -e WORKERS=1 --cpus 1 "$@" pdfrs ;;
    js*)  w=1; [ "$name" = jsN ] && w=$CORES; cpus=""; [ "$name" = js1c ] && cpus="--cpus 1"
          docker run -d --name srv --network $NET $cpus -v "$PDFJS_DIR":/app:ro -v pdfjs-node-modules:/app/node_modules:ro -w /app \
            -e NODE_ENV=production -e NODE_OPTIONS=--max-old-space-size=128 -e WORKERS=$w -e PORT=3000 node:26-alpine node bench/server.ts ;;
  esac >/dev/null
  sleep 3
}

echo "server,vus,req_s,p95_ms,fail_rate,peak_mem_mb"
for cfg in ${@:-rs js1 jsN rs1 js1c}; do
  start "$cfg"
  for v in 1 5 10 25 50 100 200; do
    docker stats --format '{{.MemUsage}}' srv > /tmp/pdfbench-mem.txt 2>/dev/null & statpid=$!
    docker run --rm --network $NET -v "$PWD":/w -w /w/bench grafana/k6:latest run -q --vus $v --duration $DURATION \
      -e URL=http://srv:3000 --summary-export /w/target-bench.json k6.js >/dev/null 2>&1
    kill $statpid 2>/dev/null; wait $statpid 2>/dev/null
    peak=$(sed 's/\x1b\[[0-9;]*[A-Za-z]//g' /tmp/pdfbench-mem.txt | awk '{ v = $1; u = v; sub(/[0-9.]+/, "", u); sub(/[A-Za-z]+/, "", v);
      m = (u == "GiB") ? v * 1024 : (u == "KiB") ? v / 1024 : v; if (m > p) p = m } END { printf "%d", p }')
    node -e '
      const m = require(process.cwd() + "/target-bench.json").metrics;
      console.log([process.argv[1], process.argv[2], m.http_reqs.rate.toFixed(0), m.http_req_duration["p(95)"].toFixed(0),
        m.http_req_failed.value, process.argv[3]].join(","))' "$cfg" $v "$peak"
  done
done
docker rm -f srv >/dev/null 2>&1
rm -f target-bench.json
