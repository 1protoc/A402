# A402-Solana: Privacy-Focused x402 Protocol Design Document

> Version: 0.4.0
> Date: 2026-04-12
> Status: Draft
> Reference: [A402 Paper](./a402.pdf) (arXiv:2603.01179v2)

---

## 1. Introduction

### 1.1 Background

x402 is an open standard that uses the HTTP `402 Payment Required` status code to integrate blockchain-based payments into web services. It is widely used as infrastructure for agentic commerce, where AI agents discover, consume, and pay for services autonomously.

Current x402 has a fundamental privacy problem: every USDC transfer is public on-chain. When an address is linked to a name service such as SNS or ENS, the buyer can be identified by third parties. In traditional commerce, outsiders do not learn what a buyer purchased; with naive x402, who paid whom and how much is public.

### 1.2 Purpose

A402-Solana follows the A402 paper architecture and implements the following on Solana:

1. **Sender anonymity**: Hide sender addresses from on-chain observers
2. **Selective disclosure**: Allow authorized auditors to decrypt sender information at provider-level granularity
3. **x402 HTTP envelope compatibility**: Preserve `HTTP 402`, `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, and `PAYMENT-RESPONSE` communication shape

### 1.3 Design Philosophy

The protocol uses a TEE as the primary trust base, matching the A402 paper.

- **Phases 1-4**: TEE-based architecture matching the paper
- **Phase 5**: Arcium MXE integration as an additional privacy layer for encrypted on-chain balances

Reasons for the TEE-first approach:

- Directly maps to the A402 architecture and is easier to verify
- Enclave code can be written in normal Rust/TypeScript, improving development speed
- Avoids early design complexity caused by stateless encrypted-computation constraints
- Arcium can be introduced later as an additional privacy layer

### 1.4 Target Environment

Initial development and verification target **Solana Devnet + AWS Nitro Enclaves-capable EC2**. Mainnet migration is considered after Phase 4.

### 1.5 Known Differences from the Paper

| Difference | A402 Paper | This Protocol | Reason |
| --- | --- | --- | --- |
| Adaptor signatures | Schnorr (secp256k1) | TEE reservation in Phases 1-2; Ed25519 adaptor signatures in Phase 3 | Solana uses Ed25519 and is incompatible with Schnorr adaptor signatures |
| Provider integration | Dedicated A402 protocol | `a402-svm-v1` scheme inside x402 HTTP envelope | Lower integration cost through HTTP 402 compatibility |
| Service Provider TEE | Provider also runs in TEE | Provider is unchanged in Phases 1-2; Provider TEE in Phase 3 | Phased rollout |
| Exec-Pay-Deliver | Cryptographically guaranteed | Not guaranteed in Phases 1-2; fully guaranteed in Phase 3 | Consequence of phased rollout |
| On-chain attestation | TEE registration on-chain | Clients/watchers verify Nitro Attestation; on-chain pins policy hash | Direct Nitro attestation verification on Solana is impractical |
| TEE runtime | Abstract TEE | AWS Nitro Enclaves | Concrete operations, key management, and recovery design |
| Receipt Watchtower | Monitoring only during challenge period | Required receipt mirror service for stale receipt challenges when enclave is down | Force-settle safety depends on watchtower availability |
| Selective disclosure | Out of scope | ElGamal-encrypted AuditRecord plus hierarchical provider-level key derivation | Added for audit requirements |

### 1.6 Companion Specifications

- Wire protocol: [a402-svm-v1-protocol.md](./a402-svm-v1-protocol.md)
- Nitro deployment / operations: [a402-nitro-deployment.md](./a402-nitro-deployment.md)

---

## 2. Privacy Model

### 2.1 Threat Model

Protected parties:

- Third parties such as on-chain observers and blockchain explorer users

Assumptions:

- **Trusted Hardware**: The TEE preserves confidentiality and integrity of code and data even if the host OS or hypervisor is compromised
- **Nitro-specific assumptions**:
  - Enclave debug mode is disabled; EIF signature and PCR0/PCR1/PCR2/PCR3/PCR8 are pinned in advance
  - Enclaves have no direct network or persistent disk; the parent instance only provides vsock relay, KMS proxy, and encrypted snapshot storage
  - KMS access is constrained by attestation document PCR conditions, so the parent instance alone cannot retrieve secrets or snapshot decryption keys
- **Adversarial parties**: Protocol participants outside the TEE may be malicious
  - Malicious clients can attempt double-spend or free usage
  - Malicious vault operators / parent instances cannot tamper with TEE code, but can block, reorder, replay, or DoS I/O
  - Malicious service providers can delay or tamper with messages
- **Network adversary**: Fully asynchronous network with eavesdropping, tampering, and delays

### 2.2 What Is Hidden

| Information | Third Party | Vault Operator / Parent | TEE Vault | Auditor (master key) | Auditor (provider key) |
| --- | --- | --- | --- | --- | --- |
| Sender address | Hidden | Hidden by TLS termination + TEE protection | Known | Known | Known for target only |
| Payment amount | Hidden | Hidden by TEE protection | Known | Known | Known for target only |
| Client balance | Hidden | Hidden by TEE protection | Known | N/A | N/A |
| Vault -> Provider transfer | Visible | Visible | Visible | Visible | Visible |
| Vault depositor set | Visible | Visible | Visible | Visible | Visible |

Compared with earlier relayer designs that handled client information in plaintext, this design terminates TLS inside the Nitro Enclave and restricts the parent instance to L4 relay. Even the vault operator cannot access client information, matching the A402 paper trust model more closely.

### 2.3 Anonymity Model

The protocol uses mixer/pool-style anonymity based on the A402 paper's privacy-preserving liquidity vault model:

```text
N participants deposit into the vault -> any vault user can pay as one of N users
-> a specific payment cannot be linked to a specific depositor
```

- Anonymity set size equals the number of vault depositors
- The initial deposit (`client -> vault`) is visible on-chain, but later per-request payments are anonymized
- Vault settlement aggregates many payments into one on-chain transaction and leaves no per-ASC trace

### 2.4 Selective Disclosure

Hierarchical key derivation controls audit granularity at the provider level:

```text
Master Auditor Secret
  |
  +-- KDF(master_secret, provider_A_address) -> Provider A ElGamal keypair
  +-- KDF(master_secret, provider_B_address) -> Provider B ElGamal keypair
  +-- KDF(master_secret, provider_C_address) -> Provider C ElGamal keypair
```

Each AuditRecord is encrypted with the ElGamal public key derived for the target provider.

| Disclosure scenario | Key shared | Decryptable scope |
| --- | --- | --- |
| Full audit | Master secret | All payments to all providers |
| Single provider audit | Provider A derived key | Payments to Provider A only |
| Multi-provider audit | Provider A/B derived keys | Payments to A and B only |

### 2.5 Exec-Pay-Deliver Atomicity

The A402 paper uses adaptor signatures to cryptographically guarantee that payment finalizes only for an executed service. This protocol reaches that property in phases.

**Phases 1-2: x402 HTTP envelope + enclave reservation model**

- Nitro Enclave verifies `PAYMENT-SIGNATURE` and finalizes internal balances after provider `/settle`
- Atomicity is **not guaranteed**: a provider could settle and fail to return a result
- This remains a provider trust-model risk until Phase 3 Provider TEE
- Privacy goals are still met because no per-client-to-provider payment appears on-chain

**Phase 3: Atomic Exchange**

- Provider also runs in a TEE and follows A402 Algorithm 2
- Ed25519 adaptor signatures provide cryptographic atomicity
- Provider cannot receive payment unless it returns the result

### 2.6 Known Privacy Gaps

- **Initial deposit visibility**: `client -> vault` deposits are public on-chain. Depositor addresses are visible as members of the anonymity set. Token-2022 Confidential Transfer can address this in the future.

---

## 3. System Architecture

### 3.1 Overview

```text
 Client                          AWS Parent Instance                    Nitro Enclave
 +-------------+            +------------------------+           +-------------------------+
 |  A402 SDK   |--TLS------>| L4 ingress relay       |--vsock-->| A402 Facilitator API    |
 | verifyAtt   |            | L4 egress relay        |<-vsock---| Vault state manager     |
 +------+------+            | KMS proxy              |           | Audit encryption        |
        |                   | Encrypted snapshot I/O |           | Solana signer           |
        | HTTP 402 retry    +----------+-------------+           | Remote Attestation      |
        v                              |                         +----------+--------------+
 +--------------+                      |                                    |
 |   Provider   |--/verify,/settle-----+                                    |
 | x402 endpoint|                                                           |
 +------+-------+                                                           |
        |                                                     TLS over L4 relay
        v                                                                   v
 +------------------+                                            +------------------+
 |  Vault Program   |<--------------settle_vault-----------------| Solana RPC       |
 |  (Anchor)        |<--------------deposit/withdraw-------------| + WebSocket      |
 | VaultConfig PDA  |                                            +------------------+
 | VaultToken PDA   |---- shared USDC pool
 | AuditRecord[]    |---- encrypted audit trail
 +--------+---------+
          |
          v
 +------------------+
 | Provider Token   |
 | Accounts (USDC)  |
 +------------------+
```

### 3.2 A402 Concept Mapping

| A402 Paper | This Protocol | Notes |
| --- | --- | --- |
| Vault (U) managed by TEE | Vault inside Nitro Enclave | Balances, ASC state, and signer are managed in enclave |
| Client (C) | Client SDK + Solana keypair | Verifies Nitro PCRs and vault signer through remote attestation |
| Service Provider (S) | x402 endpoint + custom facilitator configuration | Business API remains unchanged; payment scheme is A402-aware |
| On-chain Program (L) | Anchor program on Solana | Escrow, settlement, dispute resolution |
| Attested Runtime Policy | Nitro Enclave + governance-pinned policy hash | Solana pins the attestation policy hash |
| Adaptor Signatures | Ed25519 conditional signatures | Introduced in Phase 3 |
| Liquidity Vault | Shared Vault PDA + enclave internal ledger | Individual balances exist only in enclave |
| Batch Settlement | `settle_vault` instruction | Aggregates many ASC settlements into one tx |
| Audit Log | ElGamal-encrypted AuditRecord PDA | Encrypted in enclave, stored on-chain |
| Force Settlement | `force_settle_init` / `force_settle_finalize` | On-chain exit path when enclave fails |

### 3.3 Component Responsibilities

**Nitro Enclave:**

- Manage client balances in memory with KMS-protected snapshot/WAL
- Create, manage, and close ASCs off-chain
- Provide atomic exchange in Phase 3
- Encrypt audit records with provider-derived ElGamal keys
- Serve A402 Facilitator API (`/verify`, `/settle`, `/attestation`)
- Produce remote attestation documents
- Sign balance receipts and withdraw authorizations

**On-chain Program:**

- Escrow USDC in the vault token account
- Initialize and configure vaults
- Execute aggregate settlement submitted by the TEE
- Provide force settlement and dispute windows when enclave is unavailable
- Store encrypted audit records

**AWS Parent Instance:**

- L4 ingress relay without TLS termination
- L4 egress relay for Solana RPC and provider HTTPS
- Nitro KMS proxy
- Persistent storage for encrypted snapshot/WAL on EBS/S3

**Receipt Watchtower:**

- Store the latest `ParticipantReceipt` for each participant
- Submit `force_settle_challenge` when the enclave is down
- Store only participant, recipient ATA, free balance, locked balance, max lock expiry, and nonce; no per-purchase history

**Client SDK:**

- Verify remote attestation
- Deposit and withdraw from the vault
- Provide x402-compatible `fetch` that generates `a402-svm-v1` payloads internally
- Provide audit tooling

---

## 4. x402 Compatibility

### 4.1 Design Principle

**Preserve the HTTP 402 envelope while using an A402-specific payment scheme and facilitator.**

- Provider business APIs do not change
- Providers return `accepts[].scheme = "a402-svm-v1"` in `PAYMENT-REQUIRED`
- `PAYMENT-SIGNATURE` and `PAYMENT-RESPONSE` keep x402-compatible header shape
- A custom A402 facilitator runs inside the Nitro Enclave instead of using a generic x402 facilitator

### 4.2 Payment Flow

```text
Client SDK                Provider                  Nitro Enclave Facilitator
    |                        |                               |
    | 1. HTTP Request        |                               |
    |----------------------->|                               |
    |                        | 2. 402 PAYMENT-REQUIRED      |
    |<-----------------------|                               |
    | 3. verifyAttestation() |                               |
    |------------------------------------------------------->|
    |<-------------------------------------------------------|
    | 4. create opaque A402 payment authorization            |
    | 5. Retry + PAYMENT-SIGNATURE                           |
    |----------------------->|                               |
    |                        | 6. /verify(payload)          |
    |                        |----------------------------->|
    |                        | 7. reserve client balance    |
    |                        |<-----------------------------|
    |                        | 8. Execute service           |
    |                        | 9. /settle(result_hash, rid) |
    |                        |----------------------------->|
    |                        | 10. finalize provider credit |
    |                        |<-----------------------------|
    | 11. 200 + Response     |                               |
    |<-----------------------|                               |
    |                        | 12. later: batch settle on-chain
```

Key points:

- Clients send HTTP requests directly to providers, as with normal x402
- `PAYMENT-SIGNATURE` contains an opaque A402 authorization payload, not a raw Solana transfer transaction
- Providers still call `/verify`, execute, then `/settle`, but the counterparty is the Nitro Enclave facilitator
- On-chain settlement later sends aggregate funds from the vault to providers with `settle_vault`

### 4.3 HTTP Header Compatibility

| Header | Existing x402 | This Protocol | Difference |
| --- | --- | --- | --- |
| `Authorization` | `SIWS <token>` | `SIWS <token>` | No change |
| `PAYMENT-REQUIRED` | 402 response | 402 response | `accepts[].scheme` is `a402-svm-v1` |
| `PAYMENT-SIGNATURE` | Client-signed payment payload | Opaque A402 authorization payload | Not a raw transfer tx |
| `PAYMENT-RESPONSE` | tx hash, etc. | settle receipt / batch reference | Shape preserved, semantics are A402-specific |

---

## 5. Nitro Enclave Design

### 5.1 TEE Platform

Recommended platform: **AWS Nitro Enclaves**.

- Nitro Attestation Document enables PCR-based remote attestation
- AWS KMS can enforce `Decrypt` / `GenerateDataKey` by attestation PCR conditions
- Enclave memory is isolated from the parent EC2 instance
- Nitro Enclaves SDK, vsock, and kmstool support the implementation

Nitro constraints:

- Enclaves have **no direct network**; all I/O goes through vsock
- Enclaves have **no persistent storage**; state is stored externally as encrypted snapshot/WAL
- Debug mode must be disabled; PCR8 (EIF signature) must be part of production policy
- Parent instance is untrusted and treated only as an availability layer

Other TEEs such as Intel TDX and AMD SEV-SNP may be future targets but are not part of the initial design.

### 5.2 Internal State

TEE-only data:

```rust
struct VaultState {
    client_balances: HashMap<Pubkey, ClientBalance>,
    active_channels: HashMap<ChannelId, ChannelState>,
    vault_signer: Keypair,
    auditor_master_secret: [u8; 32],
    auditor_epoch: u32,
    pending_settlements: HashMap<Pubkey, u64>,
    receipt_nonce: u64,
    withdraw_nonce: u64,
    snapshot_seqno: u64,
    last_finalized_slot: u64,
}

struct ClientBalance {
    free: u64,
    locked: u64,
    max_lock_expires_at: i64,
    total_deposited: u64,
    total_withdrawn: u64,
}

struct ChannelState {
    channel_id: ChannelId,
    client: Pubkey,
    provider: Pubkey,
    balance: ChannelBalance,
    status: ChannelStatus,
    nonce: u64,
}
```

Nitro I/O isolation:

- **Ingress**: TLS from clients/providers is not terminated on the parent; parent forwards raw bytes to vsock and TLS terminates inside the enclave
- **Egress**: Solana RPC / provider HTTPS also flows through parent L4 egress relay while TLS is created inside the enclave
- **Persistence**: Only encrypted WAL and snapshots are stored on parent/S3/EBS

### 5.3 TEE Registration and Remote Attestation

The A402 paper performs on-chain TEE registration for both Vault and Provider. Direct Nitro AttestationDocument verification inside a Solana program is impractical due to compute cost.

This protocol pins an attestation policy hash on-chain and lets clients/watchers verify attestation off-chain.

```text
1. Client -> TEE: attestation request
2. TEE: Attest(vault_signer_pubkey || tls_pubkey || manifest_hash || snapshot_seqno)
3. TEE -> Client: attestation document + vault_signer_pubkey + attestation_policy
4. Client verifies Nitro certificate chain, PCRs, policy hash, TLS key binding, signer binding, debug-disabled state, and EIF signature
5. Client trusts `vault_signer_pubkey` for subsequent protocol messages
```

Implementation separates the KMS bootstrap attestation document from the runtime `/v1/attestation` document. The former binds the KMS response to a bootstrap recipient key. The latter binds current `snapshot_seqno` and ingress TLS public key.

On-chain trust anchors:

- `VaultConfig.vault_signer_pubkey`
- `VaultConfig.attestation_policy_hash`

Signer rotation is not in-place. A new vault is deployed and users migrate during an exit window.

### 5.4 Participant Receipts

The enclave issues signed receipts to participants after balance updates:

```rust
enum ParticipantKind {
    Client,
    Provider,
}

struct ParticipantReceipt {
    participant: Pubkey,
    participant_kind: ParticipantKind,
    recipient_ata: Pubkey,
    free_balance: u64,
    locked_balance: u64,
    max_lock_expires_at: i64,
    nonce: u64,
    timestamp: i64,
    snapshot_seqno: u64,
    vault_config: Pubkey,
}
```

Normal withdraw uses a separate replay-resistant authorization:

```rust
struct WithdrawAuthorization {
    client: Pubkey,
    recipient_ata: Pubkey,
    amount: u64,
    withdraw_nonce: u64,
    expires_at: i64,
    vault_config: Pubkey,
}
```

When the enclave fails:

- Clients can recover `free_balance` after the dispute window
- Client `locked_balance` can be recovered from the same force-settle request after `max_lock_expires_at`
- Providers use receipts with `locked_balance = 0` and `max_lock_expires_at = 0` to recover unbatched earned credit

Phase 4 requires Receipt Watchtower to prevent stale receipt abuse.

### 5.5 Atomic Exchange Protocol

**Phases 1-2: x402 envelope + enclave reservation model**

```text
1. Client computes paymentDetailsHash and request hash, then creates a a402-svm-v1 payload
2. Client retries provider request with PAYMENT-SIGNATURE
3. Provider calls enclave facilitator /verify
4. Enclave locks amount from client balance (free -> locked)
5. Provider executes service
6. Provider calls /settle
7. Enclave finalizes locked -> provider_earned
8. Enclave adds provider credit to pending_settlements for later settle_vault
```

Timeouts:

- If `/settle` does not arrive within `Delta_lock`, locked balance returns to free
- If the provider fails after `/verify`, the reservation expires and cannot be reused

Constraint: If a provider calls `/settle` but does not return the result, the client can lose payment. Phase 3 provider TEE + adaptor signatures remove this trust assumption.

**Phase 3: Ed25519 adaptor signatures**

Provider TEE is introduced and A402 Algorithm 2 is followed. The provider executes the request, creates an encrypted result and adaptor pre-signature, the vault verifies it, and payment finalization reveals the secret needed to decrypt the result. Registration binds `participantPubkey` and `participantAttestation` to the provider, and `/channel/deliver` accepts only keys matching attested registration.

### 5.6 Deposit Detection

Client deposits are executed directly on-chain, so the enclave reflects balances by watching on-chain events instead of trusting off-chain notifications. Because Nitro has no direct network, RPC connections go through parent L4 egress relay while TLS is created inside the enclave.

Commitment level: deposits are applied only after `finalized`; `processed` and `confirmed` are not sufficient.

WebSocket disconnect catch-up:

1. Detect disconnect and reconnect immediately
2. After reconnect, fetch missed deposit signatures with `getSignaturesForAddress(vault_token_account, { until: <last_processed_signature>, commitment: "finalized" })`
3. Fetch each transaction with `getTransaction(sig, { commitment: "finalized" })` and parse deposit instruction data
4. Skip transactions already recorded as `DepositApplied` in WAL
5. Apply missing deposits to `client_balances[client].free += amount` and append `DepositApplied`
6. Return `503 syncing` for `/verify` until catch-up completes

The same logic is used during enclave restart recovery.

### 5.7 Audit Record Generation

For each settlement, the TEE creates an encrypted audit record and writes it on-chain.

```rust
fn generate_audit_record(
    client: &Pubkey,
    provider: &Pubkey,
    amount: u64,
    auditor_epoch: u32,
    auditor_master_secret: &[u8; 32],
) -> AuditRecordData {
    let provider_derived_secret = kdf(auditor_master_secret, provider.as_ref());
    let provider_derived_pubkey = derive_elgamal_pubkey(&provider_derived_secret);

    let encrypted_sender = elgamal_encrypt(&provider_derived_pubkey, client.as_ref());
    let encrypted_amount = elgamal_encrypt(&provider_derived_pubkey, &amount.to_le_bytes());

    AuditRecordData {
        encrypted_sender,
        encrypted_amount,
        provider: *provider,
        timestamp: current_timestamp(),
        auditor_epoch,
    }
}
```

---

## 6. On-chain Program Design

The on-chain program stays intentionally simple because the TEE is primary: escrow, settlement, and dispute resolution. Individual client balances do not exist on-chain.

### 6.1 Account Structures

```rust
pub enum VaultStatus {
    Active = 0,
    Paused = 1,
    Migrating = 2,
    Retired = 3,
}

#[account]
pub struct VaultConfig {
    pub bump: u8,
    pub vault_id: u64,
    pub governance: Pubkey,
    pub status: u8,
    pub vault_signer_pubkey: Pubkey,
    pub usdc_mint: Pubkey,
    pub vault_token_account: Pubkey,
    pub auditor_master_pubkey: [u8; 32],
    pub auditor_epoch: u32,
    pub attestation_policy_hash: [u8; 32],
    pub successor_vault: Pubkey,
    pub exit_deadline: i64,
    pub lifetime_deposited: u64,
    pub lifetime_withdrawn: u64,
    pub lifetime_settled: u64,
}

#[account]
pub struct AuditRecord {
    pub bump: u8,
    pub vault: Pubkey,
    pub batch_id: u64,
    pub index: u8,
    pub encrypted_sender: [u8; 64],
    pub encrypted_amount: [u8; 64],
    pub provider: Pubkey,
    pub timestamp: i64,
    pub auditor_epoch: u32,
}

#[account]
pub struct ForceSettleRequest {
    pub bump: u8,
    pub vault: Pubkey,
    pub participant: Pubkey,
    pub participant_kind: u8,
    pub recipient_ata: Pubkey,
    pub free_balance_due: u64,
    pub locked_balance_due: u64,
    pub max_lock_expires_at: i64,
    pub receipt_nonce: u64,
    pub receipt_signature: [u8; 64],
    pub initiated_at: i64,
    pub dispute_deadline: i64,
    pub is_resolved: bool,
}

#[account]
pub struct UsedWithdrawNonce {
    pub bump: u8,
    pub vault: Pubkey,
    pub client: Pubkey,
    pub withdraw_nonce: u64,
}
```

The current escrow balance is read from `vault_token_account.amount`, not derived from lifetime counters.

### 6.2 PDA Seeds

| PDA | Seeds |
| --- | --- |
| VaultConfig | `[b"vault_config", governance, vault_id.to_le_bytes()]` |
| VaultTokenAccount | `[b"vault_token", vault_config]` |
| AuditRecord | `[b"audit", vault_config, batch_id.to_le_bytes(), index]` |
| ForceSettleRequest | `[b"force_settle", vault_config, participant, participant_kind]` |
| UsedWithdrawNonce | `[b"withdraw_nonce", vault_config, client, withdraw_nonce.to_le_bytes()]` |

### 6.3 Instructions

Vault management:

```text
initialize_vault(vault_id, vault_signer_pubkey, auditor_master_pubkey, attestation_policy_hash)
  -> create VaultConfig PDA + VaultTokenAccount PDA
  -> set status = Active and auditor_epoch = 0

announce_migration(successor_vault, exit_deadline)
  -> governance only
  -> status = Migrating
  -> announce migration without replacing signer in-place

pause_vault()
  -> governance only
  -> status = Paused
  -> stop new verify/settle and signer-authorized on-chain instructions during incidents

retire_vault()
  -> governance only
  -> require now >= exit_deadline
  -> status = Retired

rotate_auditor(new_auditor_master_pubkey)
  -> governance only
  -> assumes enclave already received new auditor secret through an attested channel
  -> increment auditor_epoch; old records remain decryptable only with old epoch key
```

Client operations:

```text
deposit(amount)
  -> require status == Active
  -> CPI transfer USDC: client ATA -> VaultTokenAccount
  -> increment lifetime_deposited
  -> enclave watches finalized deposit instruction and credits internal balance

withdraw(amount, withdraw_nonce, expires_at, enclave_signature)
  -> require status in {Active, Migrating} and now <= exit_deadline when Migrating
  -> verify enclave signature with vault_signer_pubkey
  -> require UsedWithdrawNonce PDA is unused
  -> CPI transfer USDC: VaultTokenAccount -> client ATA
  -> create UsedWithdrawNonce PDA
```

Settlement:

```text
settle_vault(batch_id, batch_chunk_hash, settlements)
  -> require status in {Active, Migrating} and now <= exit_deadline when Migrating
  -> signer must be vault_signer_pubkey
  -> transfer USDC from VaultTokenAccount to provider token accounts
  -> aggregate many client settlements in one tx without client details
  -> when audit is enabled, require matching record_audit in the same transaction

record_audit(batch_id, batch_chunk_hash, records)
  -> require status in {Active, Migrating} and now <= exit_deadline when Migrating
  -> signer must be vault_signer_pubkey
  -> create encrypted AuditRecord PDAs
  -> verify matching settle_vault in the same transaction through sysvar::instructions
  -> reject standalone execution
```

Vault status guard matrix:

- `Active`: allow `deposit`, `withdraw`, `settle_vault`, `record_audit`; `force_settle_*` is always available
- `Paused`: reject normal operations; allow only `force_settle_*`
- `Migrating`: reject `deposit`; allow `withdraw`, `settle_vault`, and `record_audit` until `exit_deadline`; then allow only `force_settle_*`
- `Retired`: allow only `force_settle_*` and audit reads

Batch limits from Solana transaction size:

- Solana transactions are limited to 1232 bytes
- Phase 1 `settle_vault` alone supports about 24 provider transfers per tx
- Phase 2+ atomic `settle_vault + record_audit` chunks support about 4-5 entries because AuditRecord PDA creation dominates
- Oversized batches split into multiple transactions without weakening privacy because transactions contain only Vault -> Provider transfers and no client identity

Automatic batches hold small provider credit until it reaches payout floor, and do not submit provider aggregates until `MIN_BATCH_PROVIDERS` and `MIN_ANONYMITY_WINDOW_SEC` are satisfied. `MAX_SETTLEMENT_DELAY_SEC` prioritizes liveness. Tiny single-provider tail chunks are held until the liveness deadline when possible.

Force settlement:

```text
force_settle_init(free_balance, locked_balance, max_lock_expires_at, receipt_nonce, receipt_signature, receipt_message)
  -> participant submits enclave-signed ParticipantReceipt
  -> verify Ed25519 signature
  -> decode receipt_message and check fields match instruction args and accounts
  -> create ForceSettleRequest PDA
  -> dispute_deadline = current_time + DISPUTE_WINDOW

force_settle_challenge(newer_receipt_nonce, newer_receipt_signature, newer_receipt_message)
  -> participant, Receipt Watchtower, or available enclave submits newer receipt
  -> update ForceSettleRequest fields with newer receipt

force_settle_finalize()
  -> after dispute_deadline and no valid challenge
  -> claim free balance immediately and locked balance after max_lock_expires_at
  -> require vault_token_account.amount >= claimable_now
  -> no partial payout; fail as vault_insolvent if underfunded
  -> mark resolved when both balances are zero
```

`force_settle_*` is always available and does not depend on governance pause/migration state. Trust-minimized recovery is guaranteed only for a solvent vault; insolvency is a protocol incident requiring governance/top-up response.

Ed25519 on-chain verification uses the `Ed25519Program` precompile plus `sysvar::instructions`. The transaction places the Ed25519 verification instruction before `force_settle_init`, and the vault program checks that instruction succeeded for `vault_signer_pubkey`, `receipt_message`, and `receipt_signature`.

### 6.4 AuditRecord PDA Cost

Each 222-byte AuditRecord PDA needs about 0.00159 SOL for rent exemption.

| Scale | AuditRecords | Required SOL | Notes |
| --- | --- | --- | --- |
| Small test | 100 | ~0.156 SOL | Devnet airdrop is enough |
| Medium | 10,000 | ~15.6 SOL | |
| Large | 100,000 | ~156 SOL | |

Mainnet considerations:

- Rent payer is the Nitro Enclave signer, which pays `settle_vault` / `record_audit` tx fees
- A future close-old-AuditRecord feature can reclaim rent after the audit retention period

---

## 7. Client SDK

### 7.1 API Design

The SDK preserves the existing x402 experience of `fetch -> handle 402 -> retry with PAYMENT-SIGNATURE`, while internally generating `a402-svm-v1` payloads for the Nitro Enclave.

```typescript
import { A402Client } from "@a402/client";

const client = new A402Client({
  walletKeypair,
  vaultAddress: new PublicKey("..."),
  enclaveUrl: "https://vault.example.com",
});

await client.verifyAttestation();

const res = await client.fetch("https://x402.alchemy.com/solana-mainnet/v2", {
  method: "POST",
  body: JSON.stringify({
    jsonrpc: "2.0", method: "eth_blockNumber", params: [], id: 1
  }),
});
```

Internally, the SDK:

1. Receives `PAYMENT-REQUIRED` from provider
2. Verifies Nitro Enclave attestation
3. Generates a `a402-svm-v1` payload locally
4. Retries provider with opaque payload in `PAYMENT-SIGNATURE`
5. Provider verifies/settles through the custom facilitator and returns response

### 7.2 Vault Operations

```typescript
await client.deposit(10_000_000);  // 10 USDC (6 decimals)
await client.withdraw(5_000_000);  // enclave signs; executed on-chain

const receipt = client.getLatestClientReceipt();
await client.forceSettle(receipt);  // withdraw after dispute window
```

### 7.3 Audit Tool

```typescript
import { AuditTool } from "@a402/client";

const auditor = new AuditTool(auditorMasterSecret);
const allRecords = await auditor.decryptAll(vaultAddress);
const providerRecords = await auditor.decryptForProvider(vaultAddress, providerAddress);
const exportedKey = auditor.exportProviderKey(providerAddress);
```

The exported provider key can decrypt only payments to that provider.

---

## 8. Development Phases

### Phase 1 - Nitro MVP: Vault + Custom Facilitator + Batch Settlement

**Goal**: Sender anonymity through Nitro Enclave plus x402 HTTP envelope compatibility with minimal components.

**Environment**: Solana Devnet + Nitro Enclave-capable EC2.

- Anchor program with all future-proof account fields and initial instructions: `initialize_vault`, `deposit`, `withdraw`, `settle_vault`, `pause_vault`
- Nitro Enclave for client balance management and custom facilitator (`/verify`, `/settle`)
- Parent instance ingress/egress relay, KMS proxy, encrypted snapshot storage
- Deposit detection after finalized on-chain instruction
- Remote attestation verifiable by clients
- KMS-backed snapshot/WAL recovery
- Basic SDK: deposit, withdraw, fetch, verifyAttestation
- Tests: Bankrun, local Nitro simulation, and Dev Nitro environment

**Privacy**: Sender anonymous to on-chain observers; parent cannot access plaintext payloads.

**Exec-Pay-Deliver**: Not guaranteed; provider trust model remains. HTTP envelope is x402-compatible, but payment semantics are A402-specific.

### Phase 2 - Audit Records + Selective Disclosure

**Goal**: Generate encrypted audit trails for every settlement and disclose at provider granularity.

- `record_audit` instruction and AuditRecord PDA
- ElGamal encryption inside enclave with provider-derived keys
- Hierarchical key derivation (KDF + ElGamal keypairs)
- Batch splitting for `settle_vault` and audit records
- `rotate_auditor` instruction, future-only epoch advancement
- SDK AuditTool
- Tests for encryption/decryption correctness and provider-specific disclosure

### Phase 3 - Atomic Service Channels + Provider TEE

**Goal**: A402-like off-chain high-frequency micropayments plus cryptographic atomicity.

- ASC state management inside enclave
- Provider TEE execution
- Ed25519 adaptor signatures for Exec-Pay-Deliver atomicity
- Provider key binding through registration `participantPubkey`
- Batch settlement of multiple ASCs into one tx
- Participant receipts for client/provider balances
- Full client SDK `fetch` wrapper

**Exec-Pay-Deliver**: Cryptographically guaranteed; provider cannot receive payment unless it returns the result.

### Phase 4 - Force Settlement + Dispute Resolution

**Goal**: On-chain exit path for enclave failure or migration.

- `force_settle_init`, `force_settle_challenge`, `force_settle_finalize`
- ForceSettleRequest PDA and dispute window for both clients and providers
- ParticipantReceipt verification with `Ed25519Program` precompile and `sysvar::instructions`
- `announce_migration` and exit window
- Required Receipt Watchtower for latest receipts and challenges

### Phase 5 - Arcium MXE Integration

**Goal**: Additional privacy layer with encrypted on-chain balances.

- `encrypted-ixs/`: Arcis circuits for `update_balance` and `settle_and_audit`
- Encrypted balance in ClientDeposit PDA (`[u8; 32]` ciphertext)
- Hybrid TEE + Arcium: TEE manages state, Arcium verifies encrypted state transitions on-chain
- Arcium-only balance privacy may be possible without TEE, reducing TEE dependence

### Phase 6 (Future) - Deposit Privacy

- Private deposits through Token-2022 Confidential Transfer
- Mainnet availability must be rechecked; as of 2026-04 it is available on ZK-Edge testnet
- Confidential Transfer needs seven transactions per transfer and is not ideal for high-frequency usage
- Future work hides vault depositor addresses as well

---

## 9. Project Structure

```text
a402-solana/
├── Anchor.toml
├── Cargo.toml
├── programs/
│   └── a402_vault/
├── enclave/
├── watchtower/
├── parent/
├── infra/
├── sdk/
├── middleware/
├── encrypted-ixs/
└── tests/
```

---

## 10. Verification Plan

| Phase | Verification target | Method |
| --- | --- | --- |
| Phase 1 | Nitro attestation, deposit -> enclave balance update -> settle_vault -> withdraw, and no per-client payment on-chain | Bankrun + Nitro integration |
| Phase 1 | Parent relay compromise cannot access TLS termination, secrets, or plaintext state | Adversary simulation |
| Phase 1 | Enclave restart recovers through KMS bootstrap + encrypted snapshot/WAL | Fault injection |
| Phase 2 | `settle_vault + record_audit` executes atomically and audit records are not missing on successful payment | E2E |
| Phase 2 | After `rotate_auditor`, new epoch records decrypt only with new keys while old records remain decryptable with old keys | E2E |
| Phase 3 | ASC open -> multiple off-chain requests -> batch settle into one tx | E2E + Nitro + Provider TEE |
| Phase 4 | Enclave shutdown -> ParticipantReceipt submission -> dispute window -> recover `free_balance` and later `locked_balance` | Bankrun + fault injection |
| Phase 4 | Receipt Watchtower challenges stale receipts and prevents over-withdrawal | Adversary simulation |
| Phase 4 | `force_settle_finalize` fails with `vault_insolvent` instead of partial payout when vault is underfunded | Insolvency simulation |
| Phase 4 | Migration from old vault to exit/new vault after `announce_migration` works | Migration rehearsal |
| Phase 5 | Arcium encrypted balance updates preserve balance privacy without TEE-only dependence | Arcium devnet |
