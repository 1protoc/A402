#!/usr/bin/env bash
# Start a local bitcoind regtest matching the env defaults the A402 Bitcoin
# tests expect.  Mirrors `scripts/evm/start-anvil.sh` for the EVM stack.
#
# Prerequisites:
#   - bitcoind on $PATH (e.g. `brew install bitcoin`)
#
# Defaults (override via env if needed):
#   A402_BITCOIN_RPC_URL       = http://127.0.0.1:18443
#   A402_BITCOIN_RPC_USER      = a402
#   A402_BITCOIN_RPC_PASSWORD  = a402

set -euo pipefail

if ! command -v bitcoind >/dev/null 2>&1; then
    echo "bitcoind not on PATH — install it first (brew install bitcoin)." >&2
    exit 1
fi

DATADIR="${A402_BITCOIN_DATADIR:-$HOME/.a402-bitcoin-regtest}"
RPC_PORT="${A402_BITCOIN_RPC_PORT:-18443}"
P2P_PORT="${A402_BITCOIN_P2P_PORT:-18444}"
RPC_USER="${A402_BITCOIN_RPC_USER:-a402}"
RPC_PASS="${A402_BITCOIN_RPC_PASSWORD:-a402}"

mkdir -p "$DATADIR"

echo "Starting bitcoind regtest:"
echo "  datadir         = $DATADIR"
echo "  rpcuser         = $RPC_USER"
echo "  rpcport         = $RPC_PORT"
echo "  p2pport         = $P2P_PORT"
echo "  fallbackfee     = 0.0002 BTC/kB"
echo

exec bitcoind \
    -regtest \
    -datadir="$DATADIR" \
    -rpcuser="$RPC_USER" \
    -rpcpassword="$RPC_PASS" \
    -rpcport="$RPC_PORT" \
    -port="$P2P_PORT" \
    -fallbackfee=0.0002 \
    -txindex=1 \
    -server=1 \
    -listen=1
