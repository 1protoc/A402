# A402: Privacy-Preserving x402 Payments with TEE Vaults

A402 is a privacy-preserving payment layer for x402-style paid HTTP APIs. It
keeps the developer-facing `HTTP 402 -> payment header -> retry` workflow, but
changes the payment semantics: clients pay into a TEE-backed vault, the vault
verifies and reserves off-chain balances, and providers are settled later in
batches. Public chains see vault-to-provider aggregate settlement, not the
client-to-provider payment edge.

This repository is the implementation artifact for the paper-oriented A402
system. The current codebase includes:

- a Solana Anchor vault program;
- a Rust TEE vault facilitator, with AWS Nitro and AMD SEV-SNP deployment paths;
- an untrusted parent relay and encrypted persistence layer;
- a receipt watchtower;
- TypeScript client SDK and Express middleware;
- Ethereum and Bitcoin settlement adapters;
- local, devnet, Nitro, middleware, SDK, and API tests.

## Design Goals

A402 is built around four implementation goals:

1. Preserve x402's web integration shape.
2. Hide the buyer-to-provider payment graph from public chain observers.
3. Keep private balance and request state inside a TEE-backed vault.
4. Provide force-settlement and receipt paths when the online vault is
   unavailable.

The central security assumption is that the vault runs inside a trusted
execution environment. For the Nitro deployment path, TLS terminates inside the
enclave. For the SEV-SNP path, TLS terminates inside the confidential VM. The
host, load balancer, relays, and snapshot storage are treated as untrusted
infrastructure.

## Repository Layout

```text
.
├── programs/a402_vault/          # Solana Anchor vault program
├── enclave/                      # Nitro Enclave facilitator and vault runtime
├── parent/                       # Untrusted L4 ingress/egress, KMS proxy, snapshot store
├── watchtower/                   # Receipt mirror and force-settlement watcher
├── sdk/                          # TypeScript buyer SDK
├── middleware/                   # TypeScript/Express provider middleware
├── chains/
│   ├── ethereum/contracts/       # Ethereum ASC and vault settlement contracts
│   └── bitcoin/                  # Bitcoin Taproot/PSBT vault policy notes
├── scripts/
│   ├── demo/                     # x402 vs A402 comparison demos
│   ├── devnet/                   # local/devnet bootstrap helpers
│   └── nitro/                    # Nitro prepare/build/provision/runtime scripts
├── infra/nitro/                  # Nitro Docker, systemd, Terraform, env templates
├── infra/sev-snp/                # AMD SEV-SNP VM env, systemd, attestation notes
├── docs/                         # Protocol, design, deployment, and quickstart docs
└── tests/                        # Anchor, enclave API, SDK, middleware, and e2e tests
```

## Protocol Overview

A provider returns a normal HTTP 402 response, but advertises an A402 payment
scheme such as:

- `a402-svm-v1` for Solana settlement;
- `a402-evm-v1` for Ethereum settlement;
- `a402-btc-v1` for Bitcoin settlement;
- `a402-v1` for the generic multichain envelope.

The client SDK signs an opaque A402 payment payload and retries the request with
`PAYMENT-SIGNATURE`. The provider middleware forwards the payload to the
facilitator:

```text
client
  -> provider API
  <- HTTP 402 PAYMENT-REQUIRED
client
  -> provider API + PAYMENT-SIGNATURE
provider
  -> enclave /v1/verify
provider executes service
provider
  -> enclave /v1/settle
enclave
  -> batched on-chain settlement later
```

For protocol details, see [docs/a402-svm-v1-protocol.md](./docs/a402-svm-v1-protocol.md)
and [docs/a402-solana-design.md](./docs/a402-solana-design.md).

## Components

### Solana Vault Program

`programs/a402_vault` is the Anchor program for the Solana settlement path.
It implements:

- vault initialization and lifecycle controls;
- deposits and enclave-authorized withdrawals;
- `settle_vault` batch settlement;
- atomic `record_audit` pairing for encrypted audit records;
- force-settlement receipt flow;
- ASC close-claim support.

Important files:

- [programs/a402_vault/src/lib.rs](./programs/a402_vault/src/lib.rs)
- [programs/a402_vault/src/instructions/settle_vault.rs](./programs/a402_vault/src/instructions/settle_vault.rs)
- [programs/a402_vault/src/instructions/record_audit.rs](./programs/a402_vault/src/instructions/record_audit.rs)
- [programs/a402_vault/src/instructions/force_settle_init.rs](./programs/a402_vault/src/instructions/force_settle_init.rs)

### TEE Vault Facilitator

`enclave` is the trusted A402 runtime. It can run inside AWS Nitro Enclaves or
inside an AMD SEV-SNP confidential VM. It exposes the facilitator API and owns
private state:

- `/v1/attestation`
- `/v1/verify`
- `/v1/settle`
- `/v1/cancel`
- `/v1/balance`
- `/v1/receipt`
- `/v1/withdraw-auth`
- provider registration and admin batch endpoints when enabled

It maintains client balances, reservations, provider credits, WAL state,
encrypted snapshots, audit records, ASC state, and multichain settlement queues.

Important files:

- [enclave/src/handlers.rs](./enclave/src/handlers.rs) — HTTP API
- [enclave/src/state.rs](./enclave/src/state.rs) — vault state machine
- [enclave/src/batch.rs](./enclave/src/batch.rs) — Solana and multichain batch loop
- [enclave/src/wal.rs](./enclave/src/wal.rs) — encrypted write-ahead log
- [enclave/src/snapshot.rs](./enclave/src/snapshot.rs) — encrypted snapshots
- [enclave/src/attestation.rs](./enclave/src/attestation.rs) — Nitro, SEV-SNP, and local attestation
- [enclave/src/chain_adapter.rs](./enclave/src/chain_adapter.rs) — chain target validation
- [enclave/src/multichain_settlement.rs](./enclave/src/multichain_settlement.rs) — EVM/BTC submitters

### Parent Instance

`parent` is deliberately untrusted. It does not terminate TLS and should not see
plaintext HTTP requests. It provides:

- TCP-to-vsock ingress relay;
- vsock-to-TCP egress relay;
- restricted KMS proxy;
- encrypted snapshot/WAL blob storage.

Important files:

- [parent/src/ingress_relay.rs](./parent/src/ingress_relay.rs)
- [parent/src/egress_relay.rs](./parent/src/egress_relay.rs)
- [parent/src/kms_proxy.rs](./parent/src/kms_proxy.rs)
- [parent/src/snapshot_store.rs](./parent/src/snapshot_store.rs)

### Watchtower

`watchtower` stores mirrored participant receipts and is the basis for
force-settlement monitoring. It is intentionally separate from the enclave so it
can preserve latest receipts even if the online facilitator is unavailable.

Important files:

- [watchtower/src/main.rs](./watchtower/src/main.rs)
- [watchtower/src/receipt_store.rs](./watchtower/src/receipt_store.rs)
- [watchtower/src/challenger.rs](./watchtower/src/challenger.rs)

### TypeScript SDK

`sdk` contains the buyer-side library. It provides:

- `A402Client` for x402-compatible paid fetch;
- `A402ExactScheme` for registering signers by network;
- Nitro attestation verification helpers;
- direct vault client functions for deposit, withdrawal, balance, receipt, and
  audit tooling.

Important files:

- [sdk/src/a402.ts](./sdk/src/a402.ts)
- [sdk/src/client.ts](./sdk/src/client.ts)
- [sdk/src/attestation.ts](./sdk/src/attestation.ts)
- [sdk/src/audit.ts](./sdk/src/audit.ts)

Package name:

```bash
yarn add a402-sdk
```

### Express Middleware

`middleware` contains the provider-side integration. It builds A402
`PAYMENT-REQUIRED` envelopes, verifies incoming `PAYMENT-SIGNATURE` headers
through the enclave, executes the protected route once, then settles.

Important files:

- [middleware/src/a402.ts](./middleware/src/a402.ts)
- [middleware/src/middleware.ts](./middleware/src/middleware.ts)
- [middleware/src/facilitator.ts](./middleware/src/facilitator.ts)
- [middleware/src/asc.ts](./middleware/src/asc.ts)

Package name:

```bash
yarn add a402-express
```

## Multichain Support

A402's core vault logic is chain-neutral at the payment-verification layer. The
current repository includes three settlement targets.

### Solana

Solana is the most complete settlement path. The enclave batches provider
credits into `settle_vault` instructions and pairs each settlement chunk with
encrypted audit records through `record_audit`.

### Ethereum

Ethereum support is implemented under `chains/ethereum/contracts`.

- [ASCManager.sol](./chains/ethereum/contracts/ASCManager.sol) implements the
  paper-style Standard ASC, Liquidity Vault, force-close/force-settle, and
  Schnorr verification hook modules.
- [A402VaultSettlement.sol](./chains/ethereum/contracts/A402VaultSettlement.sol)
  is a compact batch settlement contract for vault-to-provider aggregate
  payouts.

The enclave submitter in [enclave/src/multichain_settlement.rs](./enclave/src/multichain_settlement.rs)
can submit EVM batch payouts through JSON-RPC. Development deployments can use
an unlocked submitter account via `eth_sendTransaction`; production deployments
should replace this with in-enclave raw transaction signing or an attested
signer.

Required environment variables:

```bash
A402_EVM_RPC_URL=
A402_EVM_SETTLEMENT_CONTRACT=
A402_EVM_SUBMITTER=
```

### Bitcoin

Bitcoin support is represented as a Taproot/PSBT vault settlement path. The
enclave submitter creates a PSBT with:

- one `OP_RETURN` batch commitment output;
- one provider payout output per aggregated provider address;
- inputs funded and signed by the enclave-controlled Bitcoin Core wallet.

See [chains/bitcoin/a402_vault_policy.md](./chains/bitcoin/a402_vault_policy.md).

Required environment variables:

```bash
A402_BITCOIN_RPC_URL=
A402_BITCOIN_RPC_USER=
A402_BITCOIN_RPC_PASSWORD=
A402_BITCOIN_FEE_RATE=
```

## Build and Test

The expected development toolchain is:

- Rust and Cargo;
- Node.js and Yarn;
- Solana CLI;
- Anchor CLI;
- Docker for Nitro EIF builds;
- AWS CLI, Terraform, and Nitro CLI for AWS deployment.
- SEV-SNP-capable cloud/host tooling for AMD confidential VM deployment.

Common commands:

```bash
yarn install --frozen-lockfile
NO_DNA=1 anchor build
NO_DNA=1 anchor test
npm --prefix sdk run build
npm --prefix middleware run build
```

Rust package commands:

```bash
cargo test -p a402_vault
cargo test -p a402-enclave
cargo test -p a402-watchtower
cargo test -p a402-parent
```

Nitro/deployment commands:

```bash
yarn nitro:prepare
yarn nitro:build-eif
yarn nitro:provision
yarn nitro:run
yarn nitro:describe
```

Demo commands:

```bash
yarn demo:x402-seller
yarn demo:x402-buyer
yarn demo:a402-seller
yarn demo:a402-buyer
yarn demo:legacy:compare
```

## Deployment Shape

A402 has two TEE deployment shapes.

The intended Nitro deployment topology is:

```text
Internet
  -> NLB TCP/443 passthrough
  -> parent EC2 L4 ingress relay
  -> Nitro Enclave rustls/hyper facilitator
  -> parent EC2 L4 egress relay
  -> Solana / Ethereum / Bitcoin RPC, KMS, provider endpoints
```

Key deployment rules:

- TLS terminates inside the enclave.
- The parent instance is an L4 relay and storage host only.
- Vault signer plaintext must not be stored on the parent.
- WAL and snapshots are encrypted before leaving the enclave.
- KMS use should be bound to Nitro attestation policy.

For operational instructions, use:

- [docs/quickstart.md](./docs/quickstart.md)
- [docs/devnet-setup.md](./docs/devnet-setup.md)
- [docs/nitro-devnet-deploy.md](./docs/nitro-devnet-deploy.md)
- [docs/a402-sev-snp-deployment.md](./docs/a402-sev-snp-deployment.md)
- [docs/redeploy-devnet.md](./docs/redeploy-devnet.md)
- [infra/nitro/README.md](./infra/nitro/README.md)
- [infra/sev-snp/README.md](./infra/sev-snp/README.md)

## Configuration Names

The current codebase uses the A402 naming convention throughout:

- environment variables use `A402_*`;
- provider headers use `x-a402-provider-id` and `x-a402-provider-auth`;
- package names are `a402-sdk` and `a402-express`;
- the Solana program crate is `a402_vault`;
- binary crates are `a402-enclave`, `a402-parent`, and `a402-watchtower`.

## Documentation Index

- [docs/a402-solana-design.md](./docs/a402-solana-design.md) — full Solana/Nitro design
- [docs/a402-svm-v1-protocol.md](./docs/a402-svm-v1-protocol.md) — wire protocol
- [docs/a402-multichain-adapters.md](./docs/a402-multichain-adapters.md) — EVM/BTC adapter notes
- [docs/a402-nitro-deployment.md](./docs/a402-nitro-deployment.md) — Nitro runtime details
- [docs/a402-sev-snp-deployment.md](./docs/a402-sev-snp-deployment.md) — AMD SEV-SNP runtime option
- [docs/architecture.md](./docs/architecture.md) — architecture narrative
- [docs/demo-side-by-side.md](./docs/demo-side-by-side.md) — x402 vs A402 demo flow

## Current Implementation Status

The Solana vault, TEE facilitator, parent relay, watchtower, SDK, middleware,
devnet/Nitro scripts, and SEV-SNP deployment template are implemented in this
repository. Ethereum and Bitcoin settlement adapters are present as
implementation scaffolding and should be validated with local EVM and Bitcoin
regtest environments before production use.

This artifact is intended for research review and iterative protocol
development, not as a turnkey audited payment system.
