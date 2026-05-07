# A402 Protocol - Atomic Service Channel

A402 is a TEE-based Atomic service channel protocol that enables web 3.0 payment for web 2.0 services 

## Prerequisites

### System Requirements

- **AMD SEV-SNP SDK** 
- **AMD CPU supporting SEV-SNP** (production) or **QEMU/KVM** (development/testing)
- **OpenSSL** (for cryptographic operations)
- **GCC/G++** compiler (C++11 support)
- **Make** build tool
- **Node.js** and **npm** (for Ethereum blockchain integration)
- **Python 3** and **pip** (for Bitcoin blockchain integration)
- **Solidity compiler** (solc or Hardhat) - optional, for smart contracts

### Install Dependencies

```bash
# Install OpenSSL development libraries
sudo apt-get install libssl-dev

# Install build tools
sudo apt-get install build-essential

# Install Node.js and npm (for Ethereum blockchain scripts)
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt-get install -y nodejs

# Install Python 3 and pip (for Bitcoin blockchain scripts)
sudo apt-get install -y python3 python3-pip
```

## Environment Setup

### AMD SEV-SNP SDK Configuration

```bash
# Set SEV-SNP SDK path (adjust according to your installation)
export SEV_SNP_SDK=/opt/amd/sev-snp-sdk
export SEV_SNP_ARCH=x86_64

# Add SDK to PATH
export PATH=$SEV_SNP_SDK/bin:$PATH
export LD_LIBRARY_PATH=$SEV_SNP_SDK/lib:$LD_LIBRARY_PATH
```

### U-TEE Replica Configuration

Configure the number of U-TEE replicas (S-Vault instances):

```bash
export UTEE_REPLICA_COUNT=1
```

### Blockchain Configuration

**Ethereum Integration:**

```bash
cd blockchain/ethereum
npm install
```

**Bitcoin Integration:**

```bash
cd blockchain/bitcoin
pip install -r requirements.txt
```

Set Bitcoin RPC URL (optional, defaults to `http://127.0.0.1:18332`):

```bash
export BITCOIN_RPC_URL="http://user:password@127.0.0.1:18443"
```

## Building

### Build U-TEE (Vault)

```bash
cd U-TEE
make all
```

This builds both Guest VM and Host VM components:
- Guest VM: `GuestVM/utee_guest.bin`
- Host VM: `HostVM/utee_host_app`

### Build M-TEE (Service Provider)

```bash
cd M-TEE
make all
```

This builds both Guest VM and Host VM components:
- Guest VM: `GuestVM/mtee_guest.bin`
- Host VM: `HostVM/mtee_host_app`

### Build Tests

```bash
cd tests
make
```

### Clean Build Artifacts

```bash
# Clean U-TEE
cd U-TEE && make clean

# Clean M-TEE
cd M-TEE && make clean

# Clean tests
cd tests && make clean
```

## Running

### Start Host VM Mediator Service

The Host VM mediator service handles inter-VM communication:

```bash
# Start mediator service (if separate service)
# Usually integrated into Host VM applications
```

### Start U-TEE (Vault)

```bash
cd U-TEE/HostVM
./utee_host_app
```

The Host VM will automatically start and manage the Guest VM.

### Start M-TEE (Service Provider)

```bash
cd M-TEE/HostVM
./mtee_host_app
```

### Run Tests

```bash
# Run automated tests (starts U-TEE and M-TEE automatically)
cd tests
./run_tests.sh

# Run tests with M-TEE
./run_tests.sh --with-mtee

# Run simple test
./test_simple

# Run full test client
./test_client 127.0.0.1
```

## Benchmark

The `benchmark/` directory contains experimental scripts and results for performance testing.

### Test Scripts

- **`run_scalability_test.sh`**: Scalability test script that measures system performance (throughput, latency, etc.) under different request volumes
- **`run_multi_utee_test.sh`**: Multi-U-TEE coordination test script that tests performance of multiple U-TEE instances communicating with M-TEE through a coordinator

### Test Programs

- **`test_utee_mtee_distribution.cpp`**: U-TEE and M-TEE distributed test program
- **`test_multi_utee_coordination.cpp`**: Multi-U-TEE coordination test program
- **`a402_protocol_integrated.cpp`**: A402 protocol integration test
- **`benchmark_framework.h`**: Benchmark framework header file

### Test Results

- **`scalability_results_v2.json`**: Scalability test results containing performance metrics (throughput, average latency, min/max latency, etc.) for different request volumes

### Running Benchmarks

```bash
cd benchmark

# Run scalability test
./run_scalability_test.sh

# Run multi-U-TEE coordination test
./run_multi_utee_test.sh
```

### Ethereum Cost Testing

Ethereum chain cost (gas) testing scripts are located in `blockchain/ethereum/scripts/`

These scripts estimate the gas cost for various blockchain operations to help analyze on-chain transaction costs.

### Bitcoin Cost Testing

Bitcoin payment channel testing scripts are located in `blockchain/bitcoin/scripts/`


These scripts provide Bitcoin payment channel functionality including tapscript generation, transaction creation, and channel management operations.
