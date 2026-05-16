#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PID_FILE="$ROOT/data/anvil.pid"

if [[ ! -f "$PID_FILE" ]]; then
    echo "no anvil pid file at $PID_FILE"
    exit 0
fi

pid=$(cat "$PID_FILE")
if kill -0 "$pid" 2>/dev/null; then
    kill "$pid"
    echo "anvil stopped (pid $pid)"
fi
rm -f "$PID_FILE"
