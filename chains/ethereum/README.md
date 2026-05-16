# A402 — Ethereum (Foundry)

This directory is the EVM settlement layer of A402. It is a standalone Foundry
project — `forge`, `cast`, and `anvil` only. No Hardhat, no Node toolchain.

## Layout

```
chains/ethereum/
├── foundry.toml
├── remappings.txt
├── src/
│   ├── A402VaultSettlement.sol   # Liquidity Vault batch settlement
│   ├── ASCManager.sol            # Standard ASC + force-close
│   └── mocks/MockERC20.sol       # Dev/test fixture USDC
├── test/
│   └── A402VaultSettlement.t.sol # forge test cases
├── script/
│   └── Deploy.s.sol              # Local Anvil deploy script
└── deployments/                  # JSON output of forge script
```

## Prerequisites

```bash
# Install Foundry (once)
curl -L https://foundry.paradigm.xyz | bash
foundryup

# Install forge-std (once per clone)
yarn evm:install-deps
# or directly:
cd chains/ethereum && forge install foundry-rs/forge-std --no-commit
```

## Test

```bash
yarn evm:test
# or:
cd chains/ethereum && forge test -vvv
```

## Run a local chain

```bash
yarn evm:anvil          # foreground (Ctrl-C to stop)
yarn evm:anvil:bg       # background, pid file under data/anvil.pid
yarn evm:anvil:stop
```

Anvil defaults:

- RPC URL: `http://127.0.0.1:8545`
- Chain ID: `31337`
- Block time: 1s
- 10 pre-funded accounts (10 000 ETH each)
- State persisted to `chains/ethereum/anvil_state.json`

## Deploy to local Anvil

```bash
yarn evm:anvil:bg
yarn evm:bootstrap
```

This deploys `MockERC20` + `A402VaultSettlement`, prefunds the vault with 10M
mock USDC, and writes `.env.evm.generated` at the repo root so the enclave can
read:

```
A402_EVM_RPC_URL=http://127.0.0.1:8545
A402_EVM_SETTLEMENT_CONTRACT=0x...
A402_EVM_SUBMITTER=0xf39F...
A402_EVM_ASSET=0x...
```

## Wire to the enclave

The enclave submitter at
[`enclave/src/multichain_settlement.rs`](../../enclave/src/multichain_settlement.rs)
calls `eth_sendTransaction` with the Anvil-unlocked submitter address. No
private key is held in the enclave for the local dev path.

## Production note

`A402VaultSettlement` is intentionally minimal — it trusts a single submitter
EOA. Real deployments should:

- replace the submitter with an enclave-attested signer that signs raw
  transactions in-TEE, and
- gate `settleBatch` on a Nitro/SEV-SNP attestation proof or a multi-sig
  threshold.

`ASCManager` integrates the Schnorr adaptor verifier
([noot/schnorr-verify](https://github.com/noot/schnorr-verify)); the verifier
contract address must be set via constructor.
