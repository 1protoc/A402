#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== FlashPay Test Runner ==="
echo ""

if [ ! -f "$SCRIPT_DIR/test_client" ]; then
    echo "Building test client..."
    cd "$SCRIPT_DIR"
    make
fi

if [ ! -f "$PROJECT_ROOT/U-TEE/HostVM/utee_host_app" ]; then
    echo "Building U-TEE..."
    cd "$PROJECT_ROOT/U-TEE"
    make host
fi

if [ ! -f "$PROJECT_ROOT/M-TEE/HostVM/mtee_host_app" ]; then
    echo "Building M-TEE..."
    cd "$PROJECT_ROOT/M-TEE"
    make host
fi

echo "Starting U-TEE server..."
cd "$PROJECT_ROOT/U-TEE/HostVM"
./utee_host_app > /tmp/utee.log 2>&1 &
U_TEE_PID=$!

echo "Waiting for U-TEE to start..."
sleep 2

if ! kill -0 $U_TEE_PID 2>/dev/null; then
    echo "ERROR: U-TEE failed to start"
    cat /tmp/utee.log
    exit 1
fi

echo "U-TEE started (PID: $U_TEE_PID)"
echo ""

if [ "$1" == "--with-mtee" ]; then
    echo "Starting M-TEE server..."
    cd "$PROJECT_ROOT/M-TEE/HostVM"
    ./mtee_host_app > /tmp/mtee.log 2>&1 &
    M_TEE_PID=$!
    sleep 2
    echo "M-TEE started (PID: $M_TEE_PID)"
    echo ""
fi

echo "Running tests..."
cd "$SCRIPT_DIR"
./test_client 127.0.0.1
TEST_RESULT=$?

echo ""
echo "Cleaning up..."

kill $U_TEE_PID 2>/dev/null || true
if [ "$1" == "--with-mtee" ]; then
    kill $M_TEE_PID 2>/dev/null || true
fi

sleep 1

if [ $TEST_RESULT -eq 0 ]; then
    echo "=== All tests passed ==="
    exit 0
else
    echo "=== Some tests failed ==="
    exit 1
fi


