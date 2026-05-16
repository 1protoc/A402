#!/usr/bin/env bash
# Start a local Anvil node for A402 EVM development.
#
# Defaults match the dev assumptions of the enclave's multichain submitter:
#   - chain id 31337
#   - 1-second block time (fast enough for demos, slow enough to read events)
#   - state persisted to chains/ethereum/anvil_state.json so a stop/start cycle
#     keeps the deployed contracts and balances.
#
# Usage:
#   ./scripts/evm/start-anvil.sh           # foreground
#   ./scripts/evm/start-anvil.sh --bg      # background, pid in data/anvil.pid

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STATE_FILE="$ROOT/chains/ethereum/anvil_state.json"
PID_FILE="$ROOT/data/anvil.pid"
LOG_FILE="$ROOT/data/logs/anvil.log"

mkdir -p "$ROOT/data/logs"

if ! command -v anvil >/dev/null 2>&1; then
    echo "anvil not found. Install Foundry first:" >&2
    echo "  curl -L https://foundry.paradigm.xyz | bash && foundryup" >&2
    exit 127
fi

ARGS=(
    --port 8545
    --chain-id 31337
    --block-time 1
    --accounts 10
    --balance 10000
    --state "$STATE_FILE"
)

if [[ "${1-}" == "--bg" ]]; then
    if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
        echo "anvil already running (pid $(cat "$PID_FILE"))"
        exit 0
    fi
    nohup anvil "${ARGS[@]}" >"$LOG_FILE" 2>&1 </dev/null &
    echo $! >"$PID_FILE"
    echo "anvil started in background, pid $(cat "$PID_FILE"), logs: $LOG_FILE"
else
    exec anvil "${ARGS[@]}"
fi
