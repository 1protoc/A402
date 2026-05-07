# A402-Solana Nitro Deployment Specification

> Version: 0.1.0
> Date: 2026-04-12
> Status: Draft
> Companion: [a402-solana-design.md](./a402-solana-design.md)

---

## 1. Goals

This document defines the deployment, recovery, and migration specification for operating the A402-Solana facilitator and vault runtime on AWS Nitro Enclaves.

Phase 1 goals:

- Keep request and response plaintext hidden from the parent instance
- Keep the vault signer, auditor secret, and snapshot keys hidden from the parent instance
- Bind enclave identity with Nitro attestation and KMS policy
- Recover from encrypted snapshot and WAL after an enclave crash

Phase 1 non-goals:

- Multi-active enclave consensus
- Cross-region BFT replication
- Provider-side enclave deployment

---

## 2. Reference Topology

```text
                Internet
                    |
             TCP 443 passthrough
                    |
                  NLB
                    |
          +-----------------------+
          | Parent EC2 Instance   |
          |                       |
          | ingress_relay         |--vsock 8443--+
          | egress_relay          |<-vsock 9443--|
          | kms_proxy supervisor  |<-vsock 8000--|
          | snapshot_store        |<-vsock 7000--|
          +-----------------------+              |
                                                 v
                                      +--------------------+
                                      | Nitro Enclave      |
                                      | rustls/hyper       |
                                      | facilitator        |
                                      | vault state        |
                                      | Solana signer      |
                                      | KMS bootstrap      |
                                      +--------------------+
```

---

## 3. AWS Components

Minimum setup:

- 1 EC2 parent instance
- 1 Nitro Enclave
- 1 Network Load Balancer
- 1 customer-managed KMS key for seed and state unwrap
- 1 S3 bucket or encrypted EBS volume for snapshot/WAL
- 1 Solana RPC provider
- 1 Receipt Watchtower service

Recommended additions:

- CloudWatch logs and metrics
- Separate watcher instance for Solana finality and force-settle monitoring
- Second warm-standby parent instance

---

## 4. Parent / Enclave Responsibility Split

### Parent Instance

The parent instance is untrusted. Its responsibilities are limited to availability and byte transport.

Allowed responsibilities:

- Relay TCP ingress to vsock
- Relay outbound TLS byte streams from the enclave to the internet
- Start the KMS proxy process
- Store encrypted snapshot and WAL blobs
- Perform health checks and process supervision

Forbidden responsibilities:

- TLS termination
- Request body parsing
- Holding the Solana signer
- Running payment verification or settlement logic
- Holding plaintext snapshots

### Enclave

Secrets held by the enclave:

- Vault signer seed
- Auditor master secret
- Decrypted snapshot and in-memory state
- Signing context for provider/client receipts

Logic run by the enclave:

- `/attestation`, `/verify`, `/settle`, `/cancel`
- Deposit detection
- Batch construction and submission
- Receipt generation
- Snapshot/WAL encryption

### Receipt Watchtower

The Receipt Watchtower is required for Phase 4 trust-minimized asset recovery.

Responsibilities:

- Store the latest `ParticipantReceipt` for each participant
- Watch `force_settle_init`
- Submit `force_settle_challenge` when a newer receipt exists

Allowed responsibilities:

- Store receipt metadata, including `freeBalance`, `lockedBalance`, `maxLockExpiresAt`, and `nonce`
- Watch Solana and submit challenge transactions

Forbidden responsibilities:

- Facilitator signing
- Reading request bodies
- Running payment verification logic

---

## 5. Ingress Path

### 5.1 Required Property

TLS between clients/providers and the facilitator must terminate inside the enclave.

Therefore:

- Use **NLB TCP mode**
- Do **not** use ALB
- Do **not** use ACM for Nitro Enclaves with nginx on the parent in Phase 1

Rationale:

- ALB decrypts HTTP/TLS before the parent boundary
- ACM for Nitro plus nginx can isolate the private key in the enclave, but HTTP plaintext is still visible to parent nginx
- A402's privacy goal requires request path, body, and payment payload to be hidden from the parent

### 5.2 Listener Layout

- NLB: TCP/443 -> parent instance port 443
- Parent `ingress_relay`: forwards TCP/443 as a raw byte stream to vsock/8443
- Enclave: runs rustls + HTTP server on vsock/8443

---

## 6. Egress Path

Nitro Enclaves have no direct network interface, so outbound traffic goes through the parent relay.

### 6.1 Traffic Classes

- Solana RPC HTTPS
- Solana WebSocket subscriptions
- Provider callback / provider verification traffic
- KMS bootstrap traffic

### 6.2 Rules

- TLS sessions are created inside the enclave
- The parent provides only a byte pipe to destination IP/port
- Restrict outbound destinations with a parent-side firewall allowlist

Recommended allowlist:

- Configured Solana RPC endpoint
- Configured provider domains
- KMS / STS / Nitro-related AWS endpoints

---

## 7. Attestation and KMS Bootstrap

### 7.1 Build Artifacts

Deployment artifacts:

- Signed EIF image
- Enclave manifest
- Attestation policy JSON

Minimum pinned PCRs:

- `PCR0`: image measurement
- `PCR1`: kernel / bootstrap measurement
- `PCR2`: application / filesystem-related measurement
- `PCR3`: role-specific runtime inputs
- `PCR8`: EIF signing certificate measurement

### 7.2 Attestation Policy Hash

The on-chain `attestation_policy_hash` is the SHA-256 hash of canonical JSON with this shape:

```json
{
  "version": 1,
  "pcrs": {
    "0": "<hex>",
    "1": "<hex>",
    "2": "<hex>",
    "3": "<hex>",
    "8": "<hex>"
  },
  "eifSigningCertSha256": "<hex>",
  "kmsKeyArnSha256": "<hex>",
  "protocol": "a402-svm-v1"
}
```

### 7.3 KMS Keys

Phase 1 uses at least two key purposes:

- `a402-root-key`
  - Vault signer seed
  - Auditor master secret
  - Snapshot master-key wrapping

- `a402-snapshot-data-key`
  - Content encryption for snapshot / WAL blobs

### 7.4 KMS Policy Requirements

KMS key policy must be restricted with Nitro attestation condition keys.

Intent:

- The parent instance IAM role alone cannot decrypt
- KMS returns a data key only when an attested enclave provides an attestation document
- PCR sets and EIF signer values outside the allowed policy are denied

### 7.5 Bootstrap Sequence

1. Parent starts the enclave
2. Enclave generates an ephemeral bootstrap key pair
3. Enclave creates an attestation document
4. Enclave calls `Decrypt` or `GenerateDataKey` through kmstool / KMS proxy
5. KMS checks attestation conditions and binds the response to the enclave public key
6. Enclave restores the vault signer seed and snapshot key material
7. Facilitator API becomes `ready` only after snapshot/WAL recovery completes

Notes:

- The bootstrap document in step 3 binds the KMS recipient key and does not need to be identical to the `/v1/attestation` document returned to clients
- During serving, the facilitator generates a fresh runtime attestation document with NSM; `user_data` binds `vault_signer`, `attestation_policy_hash`, and `snapshot_seqno`, while `public_key` binds the ingress TLS public key

---

## 8. Persistence Model

Nitro Enclaves have no persistent disk. State persistence uses two encrypted layers:

- Encrypted WAL
- Encrypted snapshot

### 8.1 WAL Entry Types

Minimum event set:

- `DepositApplied`
- `ReservationCreated`
- `ReservationCancelled`
- `ReservationExpired`
- `SettlementCommitted`
- `ParticipantReceiptIssued`
- `ParticipantReceiptMirrored`
- `AuditorRotated`
- `BatchSubmitted`
- `BatchConfirmed`
- `MigrationAnnounced`

### 8.2 Commit Rule

Before returning a `/verify` or `/settle` response:

1. Generate the corresponding WAL entry
2. Encrypt it with the data key
3. Append it to the parent `snapshot_store`
4. Receive append ack
5. Return the success response only after the durable append

Breaking this order can create inconsistency between receipts returned to providers/clients and enclave internal state after a crash.

When issuing `ParticipantReceipt`:

1. Record `ParticipantReceiptIssued` in WAL
2. Sync the receipt to Receipt Watchtower and receive ack
3. Record `ParticipantReceiptMirrored` in WAL

Phase 4 stale receipt safety assumes this mirror step has completed durably.

### 8.3 Snapshot Rule

Recommended cadence:

- `SNAPSHOT_EVERY_N_EVENTS = 1000`
- `SNAPSHOT_EVERY_SEC = 30`

Snapshots include:

- Vault balances
- Active reservations
- Provider credit ledger
- Current auditor epoch
- Pending batch metadata
- Latest participant receipt nonce
- Last finalized Solana slot

### 8.4 Recovery Sequence

1. Load the latest complete snapshot
2. Replay WAL entries after the snapshot sequence number
3. Reconcile in-flight batches against the Solana chain
4. Deposit catch-up: refetch deposits after `last_finalized_slot` and apply missing ones
   1. Fetch deposit transaction signatures with `getSignaturesForAddress(vault_token_account, { until: <last_processed_signature>, commitment: "finalized" })`
   2. Fetch each transaction with `getTransaction(sig, { commitment: "finalized" })` and verify deposit instruction, client signer, and amount
   3. Skip transactions already recorded as `DepositApplied` in WAL
   4. Apply unrecorded deposits to `client_balances[client].free += amount` and append `DepositApplied` to WAL
   5. Update `last_finalized_slot`
5. Return `503 recovering` for `/verify` and `/settle` until ready

This logic is shared with the steady-state WebSocket disconnect/reconnect catch-up path described in `a402-solana-design.md` section 5.6.

---

## 9. Deployment Lifecycle

### 9.1 Initial Bootstrap

1. Use Terraform to create VPC, NLB, EC2, IAM, KMS, and S3/EBS resources
2. Build the signed EIF
3. Finalize PCR values and `attestation_policy_hash`
4. Pin `vault_signer_pubkey` and `attestation_policy_hash` on-chain with `initialize_vault`
5. Start the enclave and allow traffic only after bootstrap/recovery completes

### 9.2 Upgrading Enclave Code

Do not replace the signer in-place on-chain during a code upgrade.

Procedure:

1. Build the new EIF and finalize new PCR values
2. Deploy a new vault at a separate address
3. Send `announce_migration(successor_vault, exit_deadline)` to the old vault
4. Route new traffic to the new vault
5. Release old vault client/provider balances through participant force-settle or cooperative withdrawal
   - If a client receipt has `lockedBalance > 0`, that portion is recoverable after `maxLockExpiresAt`
6. Stop the old vault after the exit window

### 9.2.1 Auditor Rotation

Auditor rotation is future-only.

1. Governance submits the new auditor master secret to the enclave through an attested admin channel
2. Enclave derives and presents the new public key
3. Governance sends `rotate_auditor(new_auditor_master_pubkey)`
4. Future AuditRecords are encrypted under the new `auditor_epoch`
5. The audit side retains old epoch secrets for historical decryption

### 9.3 Warm Standby

Phase 1 HA allows active/passive only.

- Only the active enclave accepts verify / settle traffic
- Standby enclave receives no traffic and only syncs snapshot blobs
- On failover, standby completes bootstrap + recovery before switching NLB targets

Active-active is forbidden until an attested replication protocol exists.

---

## 10. Monitoring

Minimum metrics:

- Enclave bootstrap latency
- `/verify` p50 / p95 / error rate
- `/settle` p50 / p95 / error rate
- Reservation queue size
- Provider credit backlog
- Oldest unsettled provider credit age
- Snapshot lag
- WAL append latency
- Solana submission failures
- Force-settle request count
- `vault_insolvent` error count

Minimum alerts:

- Attestation drift
- KMS decrypt failure
- Recovery mode longer than 5 minutes
- Batch settlement delay greater than `MAX_SETTLEMENT_DELAY_SEC`
- Snapshot store write failure

---

## 11. Incident Response

### 11.1 Suspected Parent Compromise

Assumptions:

- Parent root compromise
- Relay process tampering
- Disk snapshot leak

Response:

1. Run `pause_vault()` to stop new verify / settle activity
2. Confirm enclave attestation and signer are unchanged
3. Start a new parent and enclave on a different host
4. Announce migration from the old vault

Expected property:

- Parent compromise alone does not leak signer seed or snapshot plaintext

### 11.2 Suspected Enclave Compromise

Assumptions:

- Attestation mismatch
- Unexpected signer
- PCR drift

Response:

1. Cut traffic immediately
2. Run `pause_vault()`
3. Deploy a new vault and announce migration
4. Ask participants to use cooperative withdrawal / force-settle
5. Confirm Receipt Watchtower can continue stale receipt challenges
6. If `vault_insolvent` occurs, do not perform partial payout; move to top-up or a separate resolution process

### 11.3 KMS Outage

Assumptions:

- A running enclave can continue
- Fresh restart is impossible

Response:

- Do not stop the active enclave
- Reduce snapshot cadence and move to write-only mode, or run `pause_vault()` if necessary

---

## 12. Security Checklist

- Use NLB TCP passthrough
- Do not terminate TLS on the parent
- Disable enclave debug mode
- Include EIF signer certificate fingerprint in the attestation policy
- Restrict KMS key policy with attestation conditions
- Require durable WAL append before success responses
- Never rotate `vault_signer_pubkey` in-place on-chain
- Bind provider auth credentials to facilitator registration
- Store snapshot/WAL with envelope encryption at all times
- Sync the latest `ParticipantReceipt` to Receipt Watchtower

---

## 13. Open Items

- Whether warm standby snapshot handoff should use S3 events or EBS snapshots
- How strictly to pin provider callback egress domains
- Whether to include AMI hash or parent role hash in the attestation policy hash
- How the egress relay should health-check long-lived WebSocket connections
