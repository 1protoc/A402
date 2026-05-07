# A402 Bitcoin Vault Policy

Bitcoin has no account contract equivalent to Solana `settle_vault`, so the
Bitcoin adapter represents the vault as a Taproot-controlled UTXO set and uses
PSBT batch payouts.

## Taproot Spend Policy

The intended vault output is a Taproot output with:

- key path: enclave wallet key for normal batched provider settlement
- script path: recovery/watchtower path for later phases

The normal settlement path is:

```text
tr(
  enclave_internal_key,
  {
    and_v(v:pk(watchtower_key), older(DISPUTE_DELAY_CSV)),
    and_v(v:pk(provider_recovery_key), older(PROVIDER_RECOVERY_CSV))
  }
)
```

Phase 1 uses the key path and commits every batch to an `OP_RETURN` output:

```text
OP_RETURN SHA256("A402-BTC-BATCH-V1" || batch_id || network || asset || settlements)
```

Provider outputs are standard P2WPKH/P2TR addresses. The PSBT is funded and
signed by the enclave-controlled Bitcoin Core wallet. The untrusted parent may
relay RPC traffic, but it never chooses recipients or amounts.

## Submitter Contract

The enclave submitter:

1. aggregates pending Bitcoin provider credits;
2. computes the batch commitment;
3. creates a PSBT with one `OP_RETURN` commitment output and provider outputs;
4. signs/finalizes inside the enclave wallet;
5. broadcasts and records the txid as the batch transaction signature.

This mirrors Solana's behavior: on-chain observers see vault-to-provider batch
outputs, not the client payment edge.
