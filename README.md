# A402: Binding Cryptocurrency Payments to Service Execution

A402 introduces the
**Atomic Service Channel (ASC)** — a new channel protocol that
moves high-frequency `HTTP 402 Payment Required` traffic off chain
and, within each off-chain round, uses a TEE-assisted Schnorr
**adaptor signature** to enforce *Exec-Pay-Deliver atomicity*:
payment is finalized **if and only if** the request is correctly
executed and the corresponding result is delivered.

An **optional** Privacy-Preserving Liquidity Vault mode further
aggregates many ASCs behind a single TEE-backed deposit, hiding
the per-channel interaction graph and capacity from on-chain
observers.

## What A402 solves

x402 fires one on-chain transfer per paid request. The result is
threefold: **(i)** O(n) on-chain cost in the request count `n`,
**(ii)** end-to-end latency bounded by block confirmation, and
**(iii)** a fully public `C_i → S_j` payment graph including
amounts, frequencies, and counterparties.

A402 addresses all three through two constructions.

### Construction 1 — Atomic Service Channel

The core protocol. An ASC is a long-lived state channel
`(C, S, Γ)` between a Client and a Service Provider, with the
TEE-protected Vault `U` acting as the **delegated channel manager**.

- **Channel lifecycle.** `openASC` locks the deposit on
  chain. State `Γ = (B, ς, k)` — where
  `B = ⟨b_c_free, b_c_locked, b_s⟩` and
  `ς ∈ {OPEN, LOCKED, PENDING, CLOSED}` — advances purely off-chain
  through state updates the parties co-sign. The chain only sees the
  channel again at cooperative `closeASC`, or one of the unilateral
  force-close paths (`C.forceCloseASC` opens an on-chain CSV dispute
  window; `S.forceCloseASC` reveals an adaptor signature on chain).
- **Exec-Pay-Deliver atomicity per round.** The
  TEE-resident `S` produces a Schnorr adaptor pre-signature
  σ̂_S = `pSign(sk_S, m, T)` over the payment message bound to
  statement `T = t·G`, where `t` is the AES key for the encrypted
  result `EncRes`. Revealing `t` (off-chain when `U` is cooperative,
  or on-chain via `S.forceCloseASC` which exposes σ_S = `Adapt(σ̂_S, t)`)
  simultaneously reveals (i) the result plaintext and (ii) a valid
  Schnorr signature accepted by the on-chain `SchnorrVerifier`. So
  `S` cannot get paid without delivering and `U` cannot get the
  result without paying.

This construction alone takes on-chain cost from O(n) per request
to O(1) per channel and decouples request latency from blockchain
confirmation.

### Construction 2 — Privacy-Preserving Liquidity Vault (*optional* mode)

A second, **orthogonal deployment mode** for the same ASCs. The
off-chain ASC protocol is unchanged; what changes is whether each ASC
has its *own* on-chain footprint, or whether many ASCs are aggregated
behind a single Vault deposit.

- Each participant `p ∈ {C, S}` deposits into the Vault once
  (`initVault`); the Vault tracks `V_p = (v_free, v_locked)` entirely
  inside its TEE.
- ASC creation / closure (`reqOpenVaultASC` / `reqCloseVaultASC`)
  becomes **purely off-chain**. The chain sees no per-channel
  `txcreate` / `txclose`; multiple ASCs managed by the same Vault are
  cryptographically indistinguishable on chain.
- Periodic `settleVault` aggregates many ASCs' final balances into a
  single on-chain transfer, revealing only the aggregate participant
  balance — no per-channel volumes, no `C ↔ S` interaction graph.
- `forceSettleVault` provides an on-chain escape hatch if the Vault
  becomes unresponsive.

### Two deployment modes — at a glance

| Mode | On-chain per request | On-chain per ASC | What an external observer sees |
|---|---|---|---|
| Naïve x402 (baseline) | 1 transfer (O(n)) | n/a | full `C → S` graph + every amount |
| **Standard ASC** | 0 | `createASC` + `closeASC` (O(1)) | per-channel `(C, S)` pair, channel capacity |
| **Liquidity Vault mode** | 0 | 0 | only `initVault` / `settleVault` aggregate flow |

Standard mode is a 1-to-1 `(C, S)` channel — useful for a long-lived
business relationship between two parties. Liquidity Vault mode
multiplexes many ASCs behind a single Vault footprint — useful for
high-volume agentic commerce where the per-channel interaction
graph must stay private.

## Three TEE roles

| Role | Symbol | Owns | Crate |
|---|---|---|---|
| Client | C | `sk_C`, signs σ_C | [client/](./client/) — Rust library, **not** TEE-bound |
| Vault | U | `sk_U`, holds Client balance off-chain, co-signs σ_U | [enclave/](./enclave/) — TEE binary (Nitro / SEV-SNP) |
| Service Provider | S | `sk_S`, runs the paid service, signs σ̂_S / σ_S | [service_provider/](./service_provider/) — independent TEE binary |

Each TEE holds an independent keypair and is admitted by the
counterparty's attestation registry (`Reg_U` on S; `Reg_S` on U) before
any channel state is shared.

## Multichain status

| Chain | Settlement | ASC | Status |
|---|---|---|---|
| Ethereum | `A402VaultSettlement.settleBatch` | `ASCManager.sol` + on-chain Schnorr verifier | ✅ 3 Anvil e2e + 38 forge tests |
| Bitcoin  | OP_RETURN-committed batch tx, in-enclave BIP-143 ECDSA | Taproot key-path + 2-leaf script tree (CSV force-close, adv-vault recovery) | ✅ 5 regtest e2e tests |
| Solana   | `programs/a402_vault::settle_vault` (Anchor) | Off-chain Ed25519 adaptor, on-chain close-claim | 🚧 Anchor + off-chain done; multi-process e2e WIP |

The same `Vault (U)` binary serves all three chains via `multichain_settlement`.

## Repository layout

```text
.
├── programs/a402_vault/   # Solana Anchor vault program
├── chains/ethereum/       # Foundry: ASCManager, A402VaultSettlement, SchnorrVerifier
├── chains/bitcoin/        # Taproot/PSBT vault policy notes
├── shared/                # a402-shared: protocol primitives (Schnorr adaptor, EVM ABI/RPC/tx, BTC P2WPKH+P2TR, ASC channel)
├── enclave/               # Vault (U) — Nitro / SEV-SNP / local-dev binary
├── service_provider/      # Service Provider (S) — independent TEE binary
├── client/                # Client (C) Rust library
├── raft/                  # 4-node Vault committee (openraft + HTTP transport)
├── parent/                # Untrusted L4 relay + KMS proxy + encrypted snapshot/WAL host
├── watchtower/            # Mirrored ParticipantReceipt store + force-settle challenger
├── scripts/{evm,btc,nitro,devnet,demo/evm-asc-atomic}/
├── infra/{nitro,sev-snp}/
└── tests/fixtures/        # JSON fixtures for Rust ↔ JS byte-identical agreement
```

## Reproduce

### Prereqs

- Rust (from `rust-toolchain.toml`), Node + Yarn, Foundry, `bitcoind` v25+
- macOS: `brew install bitcoin`; Foundry via `yarn evm:install-deps`

### A. EVM (Ethereum) — Anvil

```bash
yarn install --frozen-lockfile
yarn evm:install-deps && yarn evm:build && yarn evm:test    # 38 forge tests
yarn evm:anvil:bg
yarn evm:bootstrap                                          # writes .env.evm.generated

set -a && source .env.evm.generated && set +a
export no_proxy='127.0.0.1,localhost'; export NO_PROXY='127.0.0.1,localhost'
cargo test -p a402-service-provider --test anvil_paper_flow \
    -- --ignored --nocapture --test-threads=1

yarn evm:anvil:stop
```

Expected (3 tests pass):
```text
test anvil_paper_flow                    ... ok   # open → 3 rounds (request+pay) → close
test anvil_force_close_sp_initiated      ... ok   # SP-side forceClose with σ̂_S adapted by t
test anvil_force_close_client_initiated  ... ok   # Client-side initForceClose + finalForceClose
```

### B. Bitcoin — bitcoind regtest

```bash
yarn btc:regtest        # binds 127.0.0.1:18443 with rpcuser/pass = a402/a402

cargo test -p a402-shared --lib btc_chain::tests \
    -- --ignored --nocapture --test-threads=1

bitcoin-cli -regtest -rpcuser=a402 -rpcpassword=a402 stop
```

Expected (5 tests pass):
```text
test regtest_send_settlement          ... ok    # P2WPKH batch settlement
test regtest_send_settlement_p2tr     ... ok    # P2TR batch settlement
test regtest_asc_cooperative_close    ... ok    # Taproot key-path close
test regtest_asc_adv_vault_recovery   ... ok    # script-path leaf B (no Vault sig)
test regtest_asc_force_close_csv      ... ok    # script-path leaf A (CSV-locked) — verifies early-broadcast is rejected
```

### Workspace tests (no external chain)

```bash
cargo test --workspace --lib            # all crate unit tests
yarn evm:test                           # 38 forge tests
```
