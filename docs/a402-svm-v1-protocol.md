# A402-SVM-V1 Protocol Specification

> Version: 0.1.0
> Date: 2026-04-12
> Status: Draft
> Companion: [a402-solana-design.md](./a402-solana-design.md)

---

## 1. Scope

`a402-svm-v1` is a custom payment scheme that preserves the x402 HTTP envelope while changing payment semantics from "the client sends a direct on-chain transfer" to "a vault balance inside a Nitro Enclave is conditionally reserved and later batch-settled on-chain."

This specification defines:

- Payment details in `PAYMENT-REQUIRED` `accepts[]`
- Payment payload in the `PAYMENT-SIGNATURE` header
- Provider/facilitator APIs: `/verify`, `/settle`, `/cancel`, `/attestation`
- Payment idempotency, reservation, and batch settlement state machines

This specification does not yet define:

- Phase 3 provider-to-provider-TEE messages
- Exact Ed25519 adaptor signature transcript
- Formal interoperability with x402 extensions such as signed offers or receipts

---

## 2. Compatibility Profile

`a402-svm-v1` preserves:

- The client sends a normal HTTP request to a paid resource
- The server returns `402 Payment Required`
- The client retries with `PAYMENT-SIGNATURE`
- The server delegates verify / settle to a facilitator
- The server returns `PAYMENT-RESPONSE`

`a402-svm-v1` changes:

- `PAYMENT-SIGNATURE` does not contain a raw Solana transfer transaction
- Verify / settle targets an A402-aware facilitator, not a generic x402 facilitator
- On-chain settlement is batched instead of per request

---

## 3. Roles

- `Client`: Buyer-side agent that sends requests to a provider
- `Provider`: HTTP server that serves a paid resource
- `Facilitator`: A402-aware verifier / reserver / settler running inside a Nitro Enclave
- `ReceiptWatchtower`: Stores latest `ParticipantReceipt` values and submits stale receipt challenges when the enclave is unavailable
- `Vault Program`: Solana escrow / batch settlement / force-settle program
- `Governance`: Operator key used only for pause and migration announcement

---

## 4. Seller Identity

The default seller flow does not require prior provider registration or API key issuance. Seller middleware includes `network`, `asset.mint`, and `payTo` in each route-level `PAYMENT-REQUIRED`. On the first valid `/verify`, the facilitator auto-registers an open seller from this tuple.

Open seller identity:

```text
providerId = "payto_" || SHA-256(
  "A402-OPEN-PROVIDER-V1
" ||
  network || "
" ||
  assetMint || "
" ||
  payTo || "
"
)[0..32]
```

Constraints:

- `payTo` is the SPL token account that ultimately receives seller settlement
- Solana middleware may derive the wallet owner's USDC ATA as `payTo` when `sellerWallet` is supplied
- Open sellers are bound to `network`, `assetMint`, `payTo`, and `vault`
- If `providerId` is omitted, middleware and facilitator use the deterministic id above
- Sellers that require explicit `providerId`, mTLS, bearer/API-key auth, or ASC provider participant attestation use the advanced registration flow

Advanced `ProviderRegistration`:

```json
{
  "providerId": "prov_01JQ8V8T3V9Q8T8M8G9K0J4W7A",
  "displayName": "alchemy-solana-rpc",
  "participantPubkey": "9xQeWvG816bUx9EPfEZmP4nTqYhA6s1xY9q6m7V4sQ9N",
  "participantAttestation": {
    "document": "base64(...)"
  },
  "settlementTokenAccount": "7xKXtg2CW...",
  "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
  "assetMint": "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
  "allowedOrigins": ["https://x402.alchemy.example"],
  "authMode": "bearer",
  "authMaterial": {
    "apiKeyId": "pk_live_provider_123"
  }
}
```

Registration constraints:

- `providerId` is unique inside the facilitator
- `settlementTokenAccount` is the provider's final SPL token account
- `authMode` supports `bearer`, `api-key`, and `mtls`
- `bearer` and `api-key` register a SHA-256 hash of the provider secret and present it via `Authorization: Bearer ...` or `x-a402-provider-auth`
- `allowedOrigins` is checked against request origin during `/verify`
- Phase 3 ASC providers must have `participantPubkey` and must complete attested registration with `participantAttestation`
- `participantAttestation.document` binds `providerId`, `participantPubkey`, and `attestationPolicyHash` in signed user data
- The facilitator verifies attestation policy PCRs and confirms the policy hash matches user data

---

## 5. PAYMENT-REQUIRED Schema

Providers return the following shape as an item in `accepts[]`.

For x402 v2 compatibility, a `402 Payment Required` response places `{"accepts":[...]}` as Base64-encoded JSON in the `PAYMENT-REQUIRED` response header. The response body may mirror the same `accepts[]`.

```json
{
  "scheme": "a402-svm-v1",
  "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
  "amount": "1000000",
  "asset": {
    "kind": "spl-token",
    "mint": "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
    "decimals": 6,
    "symbol": "USDC"
  },
  "payTo": "7xKXtg2CW...",
  "providerId": "prov_01JQ8V8T3V9Q8T8M8G9K0J4W7A",
  "facilitatorUrl": "https://vault.example.com/v1",
  "vault": {
    "config": "9oX9G2xD...",
    "signer": "6MS8C3c4...",
    "attestationPolicyHash": "1a6c2f1f4f8f2a0f7a0e8d5b5a1a6d2d53b49939f4c7d9626abfce2033d5d2fe"
  },
  "paymentDetailsId": "paydet_01JQ8VB4E4X7M1K5Q7SY4P1Y7H",
  "verifyWindowSec": 60,
  "maxSettlementDelaySec": 900,
  "privacyMode": "vault-batched-v1"
}
```

Required fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `scheme` | string | Fixed value `a402-svm-v1` |
| `network` | string | CAIP-2 Solana network id |
| `amount` | string | Decimal string in atomic units |
| `asset.mint` | string | SPL token mint |
| `payTo` | string | Provider settlement token account |
| `providerId` | string | Open-seller deterministic id or registered provider id |
| `facilitatorUrl` | string | Base URL for `/verify`, `/settle`, and `/attestation` |
| `vault.config` | string | VaultConfig PDA |
| `vault.signer` | string | Enclave signer pubkey |
| `vault.attestationPolicyHash` | string | Attestation policy hash |
| `paymentDetailsId` | string | Provider-issued unique id |
| `verifyWindowSec` | integer | Seconds `/settle` may wait after verify |
| `maxSettlementDelaySec` | integer | Maximum delay before provider credit is batched on-chain |

`paymentDetailsHash` is computed by client, provider, and facilitator as:

```text
paymentDetailsHash = SHA-256(canonical_json(selected_accept_object))
```

`canonical_json` uses UTF-8, lexicographically sorted keys, no extra whitespace, decimal integer notation, and no string normalization.

---

## 6. PAYMENT-SIGNATURE Payload

The `PAYMENT-SIGNATURE` header value is Base64-encoded UTF-8 JSON. Implementations may also accept Base64URL for backward compatibility.

```json
{
  "version": 1,
  "scheme": "a402-svm-v1",
  "paymentId": "pay_01JQ8VKGW2P4M0C31Q1QKQQR4M",
  "client": "4xzJcN4h...",
  "vault": "9oX9G2xD...",
  "providerId": "prov_01JQ8V8T3V9Q8T8M8G9K0J4W7A",
  "payTo": "7xKXtg2CW...",
  "network": "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
  "assetMint": "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
  "amount": "1000000",
  "requestHash": "4b6ee3b1ff5a4f4f923ce2d2d7a6cda3dd44f5d466fb40f11bf3f5e7c4d84c22",
  "paymentDetailsHash": "0f3073c55f5016b4310f6123f2142d0f2ef758f2f2efb7e88a2f8d2a5ec7f182",
  "expiresAt": "2026-04-12T00:30:00Z",
  "nonce": "1844674407370955161",
  "clientSig": "base64(ed25519(signature))"
}
```

Required constraints:

- `paymentId` is generated uniquely by the client
- `vault` must match `vault.config`
- `payTo` must match `payment details.payTo`
- `expiresAt` must be in the future when accepted by the provider
- `nonce` must not repeat locally for the client

### 6.1 Client Signature Message

The client signs this message with Ed25519:

```text
A402-SVM-V1-AUTH
version
scheme
paymentId
client
vault
providerId
payTo
network
assetMint
amount
requestHash
paymentDetailsHash
expiresAt
nonce
```

Each line is a UTF-8 string ending in `
`. Integers are converted to decimal strings.

---

## 7. Request Hash

`requestHash` binds the paid request to the payment authorization.

```text
requestHash = SHA-256(
  "A402-SVM-V1-REQ
" ||
  METHOD || "
" ||
  ORIGIN || "
" ||
  PATH_AND_QUERY || "
" ||
  BODY_SHA256_HEX || "
" ||
  PAYMENT_DETAILS_HASH_HEX || "
"
)
```

Rules:

- `METHOD` is the uppercase HTTP method
- `ORIGIN` is `scheme://host[:port]`
- `PATH_AND_QUERY` is raw path plus raw query
- `BODY_SHA256_HEX` is SHA-256 of request body bytes
- Empty body uses SHA-256 of the empty byte string

The provider recomputes this from the received request and passes it to the facilitator during `/verify`.

---

## 8. Facilitator API

The base URL is `facilitatorUrl`.

### 8.0 Vault Status Semantics

- `Active`: allows `/verify`, `/settle`, and `/cancel`
- `Paused`: rejects `/verify`, `/settle`, and `/cancel` with `503 vault_paused`
- `Migrating`: rejects new `/verify` with `503 vault_migrating`; allows `/settle` and `/cancel` for existing reservations until `exit_deadline`
- `Retired`: rejects `/verify`, `/settle`, and `/cancel`

Providers must not continue resource handlers after `503 vault_paused` or `503 vault_migrating`.

### 8.1 `GET /v1/attestation`

Purpose:

- Clients verify Nitro Attestation
- Providers audit facilitator runtime policy

Response:

```json
{
  "vaultConfig": "9oX9G2xD...",
  "vaultSigner": "6MS8C3c4...",
  "attestationPolicyHash": "1a6c2f1f4f8f2a0f7a0e8d5b5a1a6d2d53b49939f4c7d9626abfce2033d5d2fe",
  "attestationDocument": "base64(...)"
}
```

### 8.2 `POST /v1/verify`

Authentication:

- Default open-seller flow requires no provider API key
- Advanced registered providers may use bearer/API-key headers or mTLS
- Bearer mode identifies the provider with `x-a402-provider-id` or `X-A402-Provider-Id`

Request:

```json
{
  "paymentPayload": { "...": "..." },
  "paymentDetails": { "...": "..." },
  "requestContext": {
    "method": "POST",
    "origin": "https://x402.alchemy.example",
    "pathAndQuery": "/solana-mainnet/v2",
    "bodySha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a"
  }
}
```

The facilitator must verify:

1. Open-seller identity matches details/payload, or provider auth matches advanced registration
2. `paymentDetails.scheme == "a402-svm-v1"`
3. `paymentDetails.verifyWindowSec` is positive
4. `paymentDetailsHash` matches
5. `requestHash` matches recomputation from `requestContext`
6. `clientSig` is valid
7. `expiresAt` is in the future
8. `providerId`, `payTo`, `assetMint`, `network`, and `vault` match identity / registration / vault config
9. Client `free_balance >= amount`
10. `paymentId` is unused or is an idempotent replay for the same request
11. Vault status is `Active`

Successful `/verify` side effects:

- `free_balance -= amount`
- `locked_balance += amount`
- `reservationExpiresAt = verified_at + verifyWindowSec`
- Reservation state becomes `RESERVED`
- `ReservationCreated` is appended to encrypted WAL durably before responding

### 8.3 `POST /v1/settle`

Authentication is the same as `/verify`; the default open-seller flow binds settlement to the seller identity from the verification.

Request:

```json
{
  "verificationId": "ver_01JQ8VQ6M1MS4KTW46ZF4GJKF3",
  "resultHash": "a9cd98f7b4c3c59e4d5f6f0d215b0bb7f08933f5d6b8e0c5f9893f6ce6d033bd",
  "statusCode": 200
}
```

Successful `/settle` side effects:

- Reservation state becomes `SETTLED_OFFCHAIN`
- `locked_balance -= amount`
- Provider credit ledger increases by `amount`
- Provider `ParticipantReceipt` is issued
- `SettlementCommitted` is appended to encrypted WAL durably before responding

`/settle` succeeds only while the reservation is `RESERVED` and `now <= reservationExpiresAt`. Expired reservations transition immediately to `EXPIRED`, release locked balance back to free balance, and return `reservation_expired`.

### 8.4 Provider Single-Execution Rule

Providers must treat `verificationId` as a single-use execution capability.

Recommended provider states:

- `VERIFIED_UNSERVED`
- `EXECUTING`
- `SERVED_SUCCESS`
- `SERVED_ERROR`

Rules:

1. Only one handler execution may start for the same `verificationId`
2. A duplicate while `EXECUTING` returns `409 duplicate_execution_in_flight` or waits for the same in-flight result
3. A duplicate after `SERVED_SUCCESS` / `SERVED_ERROR` returns the original HTTP status, body, and `PAYMENT-RESPONSE`
4. Clustered deployments must use shared execution cache or sticky routing by `verificationId`

### 8.5 `POST /v1/cancel`

Purpose: the provider explicitly releases a reservation before service execution.

Authentication is the same as `/verify`. The facilitator records the `verificationId` to `providerId` binding when returning `/verify`; `/cancel` returns `403 provider_mismatch` if the authenticated provider does not match that binding.

Request:

```json
{
  "verificationId": "ver_01JQ8VQ6M1MS4KTW46ZF4GJKF3",
  "reason": "upstream_unavailable"
}
```

Response:

```json
{
  "ok": true,
  "cancelledAt": "2026-04-12T00:00:05Z"
}
```

### 8.6 PAYMENT-RESPONSE Schema

The provider includes at least this Base64-encoded JSON in the `PAYMENT-RESPONSE` header:

```json
{
  "scheme": "a402-svm-v1",
  "paymentId": "pay_01JQ8VKGW2P4M0C31Q1QKQQR4M",
  "verificationId": "ver_01JQ8VQ6M1MS4KTW46ZF4GJKF3",
  "settlementId": "set_01JQ8VV2SEQQFG28M0WSTC3Q59",
  "batchId": null,
  "txSignature": null,
  "participantReceipt": "base64(enclave-signed-provider-participant-receipt)"
}
```

Meaning:

- `batchId == null` and `txSignature == null`: off-chain settlement completed, on-chain batch pending
- After batch completion, providers can query `batchId` and `txSignature`
- `participantReceipt` is evidence for provider force-settle

---

## 9. State Machine and Idempotency

States per `paymentId`:

- `UNSEEN`
- `RESERVED`
- `CANCELLED`
- `EXPIRED`
- `SETTLED_OFFCHAIN`
- `BATCHED_ONCHAIN`

Transitions:

```text
UNSEEN --verify--> RESERVED
RESERVED --cancel--> CANCELLED
RESERVED --timeout--> EXPIRED
RESERVED --settle--> SETTLED_OFFCHAIN
SETTLED_OFFCHAIN --batch confirmed--> BATCHED_ONCHAIN
```

Idempotency rules:

- `/verify` retries with the same `paymentId`, `requestHash`, and `paymentDetailsHash` return the same `verificationId`
- Reusing the same `paymentId` for different request binding returns `409 payment_id_reused`
- `/settle` retries after `SETTLED_OFFCHAIN` return the same `settlementId`
- `/settle` on `CANCELLED` or `EXPIRED` payments is rejected
- Duplicate provider requests for the same `verificationId` must not trigger new execution

---

## 10. Batch Settlement

The facilitator tracks off-chain credit per provider and pays it on-chain through `settle_vault`.

### 10.1 Batch Trigger

Phase 1 recommended values:

- `BATCH_WINDOW_SEC = 120` (`A402_BATCH_WINDOW_SEC`)
- `MAX_SETTLEMENT_DELAY_SEC = 900`
- `MAX_SETTLEMENTS_PER_TX = 20`
- `JITTER_SEC = 0..30`

A batch triggers when:

1. Batch window elapsed
2. Pending provider count reached `MAX_SETTLEMENTS_PER_TX`
3. Oldest batch-eligible settlement reached `MAX_SETTLEMENT_DELAY_SEC`

### 10.2 Privacy Rules

- Never settle each request on-chain individually
- Mix multiple providers in the same batch whenever possible
- Select pending credit round-robin across providers so one provider does not monopolize a batch
- Defer automatic inclusion for credits below a payout floor, unless `MAX_SETTLEMENT_DELAY_SEC` has been reached
- Add jitter to batch submit time
- Return off-chain receipt at `/settle` success before on-chain arrival
- `MIN_ANONYMITY_WINDOW_SEC = 300` for public Phase 1 default: each settlement must age before automatic batching
- `MIN_BATCH_PROVIDERS = 2` for public Phase 1 default: if too few providers are present, wait until liveness deadline for more provider credits

### 10.3 Batch Receipts

After confirmation, the facilitator records `settlementId -> batchId -> txSignature` for audits and disputes.

---

## 11. Failure Semantics

- Client receipts carry `freeBalance`, `lockedBalance`, and `maxLockExpiresAt`
- Provider receipts have `lockedBalance = 0` and `maxLockExpiresAt = 0`
- If the provider fails after `/verify`, the reservation expires after `verifyWindowSec` and locked balance returns to client free balance
- If the HTTP response is lost after `/settle`, the provider retries `/settle` with the same `verificationId` and receives the same `settlementId`
- After enclave crash, encrypted snapshot + WAL recover state; unbatched provider credit is force-settleable by provider receipt
- Clients can recover `freeBalance` after the dispute window and locked portions after `maxLockExpiresAt`
- ReceiptWatchtower must hold the latest receipts for stale receipt challenges
- If a paired audit chunk fails during on-chain batch submission, the entire Solana transaction rolls back and provider credit remains `SETTLED_OFFCHAIN`

---

## 12. Security Invariants

- On-chain observers cannot see direct `client -> provider` correspondence
- Parent instance cannot read payment payloads, request bodies, secret keys, or vault balances
- Providers cannot obtain credit without facilitator verify
- The same `paymentId` cannot be reused for a different request
- The facilitator must not return verify / settle success before durable WAL write
- When audit mode is enabled, each settlement chunk must appear in the same transaction as the matching audit chunk
- During vault unavailability, participant receipts must allow client/provider balance recovery
- A client's locked portion must become recoverable after its receipt-bound `maxLockExpiresAt`
- Receipt-based recovery assumes at least one honest available ReceiptWatchtower and a solvent vault
- In Phase 3 ASC, `providerPubkey` in `/channel/deliver` must match registration `participantPubkey`; providers without registered `participantPubkey` cannot open channels

---

## 13. Open Items

- Whether provider auth should standardize on `bearer` or `mtls`
- Whether `requestHash` should require signed offers / payment identifier extension
- How Phase 3 maps `verificationId` to ASC `rid`
