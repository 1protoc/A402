# A402 Multichain Adapter Notes

This document records the first multichain extension of the A402/A402 vault.
The goal is to keep the existing x402-compatible Nitro vault flow while allowing
providers to settle to Solana, Ethereum, or Bitcoin targets.

## Scope

The current adapter layer is a protocol, state-model, and submitter extension:

- Solana providers continue to use `a402-svm-v1`.
- Ethereum providers may use `a402-evm-v1` with CAIP-2 networks such as
  `eip155:1` and an ERC-20 address or `eth/native` asset id.
- Bitcoin providers may use `a402-btc-v1` with networks such as
  `bitcoin:regtest` or `bip122:*` and `btc/native` as the asset id.
- `a402-v1` is reserved as the generic multichain wire scheme.

The enclave still uses the existing vault balance and Ed25519 client signature
model for HTTP payment verification. Non-Solana provider credits are tracked
separately and are now picked up by the multichain submitter during
`/v1/admin/fire-batch` or the automatic batch loop.

## Ethereum

Ethereum support lives in `chains/ethereum/contracts/ASCManager.sol`.

It follows the paper modules:

- Standard ASC: `createASC`, `closeASC`, `initForceClose`,
  `challengeForceClose`, `finalForceClose`, `forceClose`.
- Liquidity Vault: `initVault`, `settleVault`, `settleVaultBatch`,
  `initForceSettle`, `finalForceSettle`.
- Schnorr/adaptor verification hook: `verifySchnorr`, backed by an external
  verifier compatible with noot/schnorr-verify.

The enclave submitter uses the packed `settleBatch` entrypoint for batched
provider payouts. Configure:

- `A402_EVM_RPC_URL`
- `A402_EVM_SETTLEMENT_CONTRACT`
- `A402_EVM_SUBMITTER`

The current submitter sends `eth_sendTransaction`, so devnets should expose an
unlocked enclave submitter account or a local signer endpoint that accepts that
RPC method. Production should replace this with in-enclave EVM transaction
signing or a KMS-backed signer bound to Nitro attestation.

## Bitcoin

Bitcoin support lives in `enclave/src/multichain_settlement.rs` and
`chains/bitcoin/a402_vault_policy.md`.

The submitter creates a PSBT with:

- one `OP_RETURN` batch commitment output;
- one provider payout output per aggregated provider address;
- wallet-funded inputs from the enclave-controlled Bitcoin Core wallet.

Configure:

- `A402_BITCOIN_RPC_URL`
- `A402_BITCOIN_RPC_USER`
- `A402_BITCOIN_RPC_PASSWORD`
- `A402_BITCOIN_FEE_RATE`

## Settlement Target Model

Each provider registration now stores both the legacy Solana fields and a
normalized multichain target:

- `network`: CAIP-2 style network id.
- `asset_id`: SPL mint, ERC-20 address, `eth`, `btc`, or `native`.
- `settlement_address`: token account, EVM address, or Bitcoin address.
- `chain_kind`: `solana`, `ethereum`, or `bitcoin`.

For Solana providers, the legacy `settlement_token_account` and `asset_mint`
fields remain authoritative so existing batching keeps working.

## Follow-up Work

- Replace EVM devnet `eth_sendTransaction` flow with in-enclave raw transaction
  signing.
- Add Bitcoin recovery/watchtower script-path tests once Phase 4 force-settle
  receipts are fully multichain.
- Add integration tests with EVM localnet and Bitcoin regtest once the submitters
  are connected.
