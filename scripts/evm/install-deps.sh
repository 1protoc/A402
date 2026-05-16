#!/usr/bin/env bash
# Install Solidity test dependencies (forge-std).
#
# Works whether or not the repo root is a git repository — `forge install`
# requires git submodules, so we fall back to a plain `git clone` when the
# project is not under git control.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIB_DIR="$ROOT/chains/ethereum/lib"
TARGET="$LIB_DIR/forge-std"

mkdir -p "$LIB_DIR"

if [[ -d "$TARGET/.git" ]]; then
    echo "forge-std already present at $TARGET"
    exit 0
fi

if [[ -d "$ROOT/.git" ]]; then
    echo "Repo is a git repo — using forge install..."
    cd "$ROOT/chains/ethereum"
    forge install foundry-rs/forge-std
else
    echo "Repo is NOT a git repo — falling back to plain git clone..."
    git clone --depth=1 https://github.com/foundry-rs/forge-std.git "$TARGET"
fi

echo "Done. forge-std installed at $TARGET"
