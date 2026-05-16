//! Bitcoin batch settlement transaction builder.
//!
//! Layout:
//!
//!   inputs:  one or more P2WPKH UTXOs owned by the Vault
//!   outputs:
//!     [0]  OP_RETURN  →  batch commitment hash (32 bytes)
//!     [1..1+N]        →  one P2WPKH payout per aggregated provider
//!     [1+N]           →  P2WPKH change back to Vault
//!
//! Signing happens in-process with `secp256k1` against a BIP-143 sighash
//! (`SighashCache::p2wpkh_signature_hash`). The vault private key never
//! leaves the enclave; bitcoind only sees the final raw transaction via
//! `sendrawtransaction`.

use bitcoin::address::Address;
use bitcoin::ecdsa::Signature as BtcEcdsaSig;
use bitcoin::hashes::Hash;
use bitcoin::key::{CompressedPublicKey, Keypair, TapTweak, UntweakedPublicKey, XOnlyPublicKey};
use bitcoin::script::{Builder, PushBytesBuf};
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, Prevouts, SighashCache, TapSighashType};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Txid,
    Witness,
};
use thiserror::Error;

/// A single payout in the batch.
#[derive(Debug, Clone)]
pub struct Payout {
    pub address: Address,
    pub amount_sat: u64,
}

/// A UTXO owned by the Vault that we'll spend. The signing path requires
/// the previous output's `value` (BIP-143 sighash binds the input value).
#[derive(Debug, Clone)]
pub struct VaultUtxo {
    pub txid: Txid,
    pub vout: u32,
    pub value_sat: u64,
}

/// Vault's in-enclave Bitcoin signer. Holds raw private key material; do
/// not serialize. Construct from a TEE-bound seed (e.g. KMS unwrap).
pub struct BtcKeys {
    pub secret: SecretKey,
    pub public: CompressedPublicKey,
    pub network: Network,
}

impl std::fmt::Debug for BtcKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BtcKeys")
            .field("public", &self.public)
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

impl BtcKeys {
    pub fn from_bytes(bytes: &[u8; 32], network: Network) -> Result<Self, BtcError> {
        let secret = SecretKey::from_slice(bytes).map_err(BtcError::Key)?;
        let secp = Secp256k1::signing_only();
        let pk = secret.public_key(&secp);
        let public = CompressedPublicKey::from_slice(&pk.serialize())
            .map_err(|e| BtcError::Other(format!("CompressedPublicKey::from_slice: {e}")))?;
        Ok(Self {
            secret,
            public,
            network,
        })
    }

    pub fn from_hex(hex_priv: &str, network: Network) -> Result<Self, BtcError> {
        let stripped = hex_priv.strip_prefix("0x").unwrap_or(hex_priv);
        let raw = hex::decode(stripped).map_err(|e| BtcError::Other(format!("hex: {e}")))?;
        if raw.len() != 32 {
            return Err(BtcError::Other(format!(
                "secret must be 32 bytes, got {}",
                raw.len()
            )));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&raw);
        Self::from_bytes(&buf, network)
    }

    /// P2WPKH address derived from the public key under the configured network.
    pub fn p2wpkh_address(&self) -> Address {
        Address::p2wpkh(&self.public, self.network)
    }

    /// Internal x-only key for Taproot (BIP-340/341). `public.0` already
    /// holds a compressed secp256k1 pubkey; we just drop the parity byte.
    pub fn taproot_internal_xonly(&self) -> XOnlyPublicKey {
        let (xonly, _parity) = self.public.0.x_only_public_key();
        xonly
    }

    /// Key-only P2TR address — no script tree (Q = P + H(P) * G with empty
    /// merkle root, the BIP-341 "key-path-only" form). Slice 4B will add a
    /// `p2tr_address_with_scripts(...)` variant for the Bitcoin ASC
    /// channel's cooperative-vs-force-close script branches.
    pub fn p2tr_address(&self) -> Address {
        let secp = Secp256k1::verification_only();
        let internal: UntweakedPublicKey = self.taproot_internal_xonly();
        Address::p2tr(&secp, internal, None, self.network)
    }
}

#[derive(Debug, Error)]
pub enum BtcError {
    #[error("secp256k1 key: {0}")]
    Key(bitcoin::secp256k1::Error),
    #[error("sighash: {0}")]
    Sighash(String),
    #[error("amount conservation failed: inputs {input_sat} < outputs {output_sat} + fee {fee_sat}")]
    InsufficientFunds {
        input_sat: u64,
        output_sat: u64,
        fee_sat: u64,
    },
    #[error("op_return payload must be ≤ 80 bytes, got {0}")]
    OpReturnTooLong(usize),
    #[error("at least one input is required")]
    NoInputs,
    #[error("at least one payout is required")]
    NoPayouts,
    #[error("invalid network for address {0}: expected {1:?}")]
    AddressNetworkMismatch(String, Network),
    #[error("{0}")]
    Other(String),
}

/// Builds + signs the batch settlement transaction.
///
/// `change_address` must be the Vault's P2WPKH address (under the same
/// network as the inputs); we don't pay change to anyone else.
///
/// `batch_commit_hash` is the 32-byte digest we publish in the OP_RETURN.
/// The paper recommends `sha256d(provider_list || amounts || batch_id)`;
/// we don't enforce a particular shape here, just length ≤ 80 bytes.
pub fn build_settlement_tx(
    batch_commit_hash: &[u8; 32],
    payouts: &[Payout],
    utxos: &[VaultUtxo],
    change_address: &Address,
    fee_sat: u64,
    keys: &BtcKeys,
) -> Result<Transaction, BtcError> {
    if utxos.is_empty() {
        return Err(BtcError::NoInputs);
    }
    if payouts.is_empty() {
        return Err(BtcError::NoPayouts);
    }

    if change_address.address_type().is_none() {
        return Err(BtcError::AddressNetworkMismatch(
            change_address.to_string(),
            keys.network,
        ));
    }

    let total_in: u64 = utxos.iter().map(|u| u.value_sat).sum();
    let total_out: u64 = payouts.iter().map(|p| p.amount_sat).sum();
    if total_in < total_out + fee_sat {
        return Err(BtcError::InsufficientFunds {
            input_sat: total_in,
            output_sat: total_out,
            fee_sat,
        });
    }
    let change_sat = total_in - total_out - fee_sat;

    // -- assemble outputs --
    let mut tx_outs = Vec::with_capacity(payouts.len() + 2);

    // [0] OP_RETURN commitment
    let mut payload = PushBytesBuf::new();
    payload
        .extend_from_slice(batch_commit_hash)
        .map_err(|e| BtcError::Other(format!("op_return push: {e}")))?;
    let op_return_script = Builder::new()
        .push_opcode(bitcoin::opcodes::all::OP_RETURN)
        .push_slice(&payload)
        .into_script();
    tx_outs.push(TxOut {
        value: Amount::ZERO,
        script_pubkey: op_return_script,
    });

    // [1..1+N] payouts
    for p in payouts {
        tx_outs.push(TxOut {
            value: Amount::from_sat(p.amount_sat),
            script_pubkey: p.address.script_pubkey(),
        });
    }

    // [last] change back to vault — only when above dust (546 sats threshold
    // for P2WPKH is conservative; for simplicity drop change below 1000).
    if change_sat >= 1_000 {
        tx_outs.push(TxOut {
            value: Amount::from_sat(change_sat),
            script_pubkey: change_address.script_pubkey(),
        });
    }

    // -- assemble inputs (unsigned scaffold first; signatures fill in below) --
    let tx_ins: Vec<TxIn> = utxos
        .iter()
        .map(|u| TxIn {
            previous_output: OutPoint {
                txid: u.txid,
                vout: u.vout,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        })
        .collect();

    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: tx_ins,
        output: tx_outs,
    };

    // -- sign every input (BIP-143 P2WPKH) --
    let secp = Secp256k1::signing_only();
    let pubkey_bytes = keys.public.to_bytes();
    let signer_pk = PublicKey::from_slice(&pubkey_bytes)
        .map_err(|e| BtcError::Other(format!("PublicKey::from_slice: {e}")))?;
    let vault_address = keys.p2wpkh_address();
    let script_code = vault_address.script_pubkey();

    let mut cache = SighashCache::new(&mut tx);
    let mut witnesses: Vec<Witness> = Vec::with_capacity(utxos.len());
    for (idx, utxo) in utxos.iter().enumerate() {
        let sighash = cache
            .p2wpkh_signature_hash(
                idx,
                &script_code,
                Amount::from_sat(utxo.value_sat),
                EcdsaSighashType::All,
            )
            .map_err(|e| BtcError::Sighash(format!("p2wpkh_signature_hash[{idx}]: {e}")))?;
        let msg = Message::from_digest_slice(sighash.as_byte_array())
            .map_err(|e| BtcError::Sighash(format!("Message::from_digest: {e}")))?;
        let raw_sig = secp.sign_ecdsa(&msg, &keys.secret);
        let signature = BtcEcdsaSig {
            signature: raw_sig,
            sighash_type: EcdsaSighashType::All,
        };
        let mut w = Witness::new();
        w.push(signature.to_vec());
        w.push(signer_pk.to_bytes());
        witnesses.push(w);
    }

    for (idx, w) in witnesses.into_iter().enumerate() {
        tx.input[idx].witness = w;
    }

    Ok(tx)
}

/// Same shape as [`build_settlement_tx`] but spends **P2TR key-path**
/// inputs and signs them with BIP-341 Schnorr.
///
/// Differences from the P2WPKH variant:
///   - Inputs must be P2TR outputs owned by `keys` (i.e. funded at
///     `keys.p2tr_address()`).
///   - Sighash is `taproot_key_spend_signature_hash` over `Prevouts::All`
///     (Taproot binds every prevout, not just the spent one).
///   - Signature is a 64-byte BIP-340 Schnorr produced by the
///     **tap-tweaked** keypair (`Q = P + H_TapTweak(P) * G` with empty
///     merkle root).
///   - Witness is the single 64-byte sig (no pubkey, no sighash byte —
///     `TapSighashType::Default` is implicit).
///
/// The output layout (OP_RETURN + N payouts + change) stays identical
/// — payout addresses can mix P2WPKH and P2TR freely.
pub fn build_settlement_tx_p2tr(
    batch_commit_hash: &[u8; 32],
    payouts: &[Payout],
    utxos: &[VaultUtxo],
    change_address: &Address,
    fee_sat: u64,
    keys: &BtcKeys,
) -> Result<Transaction, BtcError> {
    if utxos.is_empty() {
        return Err(BtcError::NoInputs);
    }
    if payouts.is_empty() {
        return Err(BtcError::NoPayouts);
    }

    let total_in: u64 = utxos.iter().map(|u| u.value_sat).sum();
    let total_out: u64 = payouts.iter().map(|p| p.amount_sat).sum();
    if total_in < total_out + fee_sat {
        return Err(BtcError::InsufficientFunds {
            input_sat: total_in,
            output_sat: total_out,
            fee_sat,
        });
    }
    let change_sat = total_in - total_out - fee_sat;

    // -- outputs (identical to P2WPKH variant) --
    let mut tx_outs = Vec::with_capacity(payouts.len() + 2);

    let mut payload = PushBytesBuf::new();
    payload
        .extend_from_slice(batch_commit_hash)
        .map_err(|e| BtcError::Other(format!("op_return push: {e}")))?;
    let op_return_script = Builder::new()
        .push_opcode(bitcoin::opcodes::all::OP_RETURN)
        .push_slice(&payload)
        .into_script();
    tx_outs.push(TxOut {
        value: Amount::ZERO,
        script_pubkey: op_return_script,
    });
    for p in payouts {
        tx_outs.push(TxOut {
            value: Amount::from_sat(p.amount_sat),
            script_pubkey: p.address.script_pubkey(),
        });
    }
    if change_sat >= 1_000 {
        tx_outs.push(TxOut {
            value: Amount::from_sat(change_sat),
            script_pubkey: change_address.script_pubkey(),
        });
    }

    // -- inputs (placeholder witness; filled below) --
    let tx_ins: Vec<TxIn> = utxos
        .iter()
        .map(|u| TxIn {
            previous_output: OutPoint {
                txid: u.txid,
                vout: u.vout,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        })
        .collect();
    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: tx_ins,
        output: tx_outs,
    };

    // -- per-input prevouts for Taproot sighash --
    // Every Vault input is a P2TR output at `keys.p2tr_address()`, so the
    // scriptPubKey is the same across all inputs we own.
    let vault_p2tr_addr = keys.p2tr_address();
    let vault_p2tr_script = vault_p2tr_addr.script_pubkey();
    let prevouts: Vec<TxOut> = utxos
        .iter()
        .map(|u| TxOut {
            value: Amount::from_sat(u.value_sat),
            script_pubkey: vault_p2tr_script.clone(),
        })
        .collect();

    // -- BIP-341 tap-tweak the keypair (empty merkle root for key-only) --
    let secp = Secp256k1::new();
    let untweaked = Keypair::from_secret_key(&secp, &keys.secret);
    let tweaked = untweaked.tap_tweak(&secp, None).to_inner();

    let mut cache = SighashCache::new(&mut tx);
    let mut witnesses: Vec<Witness> = Vec::with_capacity(utxos.len());
    for (idx, _utxo) in utxos.iter().enumerate() {
        let sighash = cache
            .taproot_key_spend_signature_hash(
                idx,
                &Prevouts::All(&prevouts),
                TapSighashType::Default,
            )
            .map_err(|e| BtcError::Sighash(format!("taproot_key_spend[{idx}]: {e}")))?;
        let msg = Message::from_digest_slice(sighash.as_byte_array())
            .map_err(|e| BtcError::Sighash(format!("Message::from_digest: {e}")))?;
        let sig = secp.sign_schnorr_no_aux_rand(&msg, &tweaked);
        // 64-byte signature; TapSighashType::Default → omit sighash flag byte.
        let mut w = Witness::new();
        w.push(sig.as_ref());
        witnesses.push(w);
    }
    for (idx, w) in witnesses.into_iter().enumerate() {
        tx.input[idx].witness = w;
    }

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::hashes::sha256d;

    /// Vector key — Anvil-style deterministic 32-byte secret for tests only.
    const TEST_VAULT_PRIV: &str =
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    #[test]
    fn build_settlement_tx_round_trip_offline() {
        // Smoke test: build a 2-payout settlement tx and inspect its shape
        // without touching bitcoind. The regtest test in `btc_chain` does
        // the full send / mine path.
        let keys = BtcKeys::from_hex(TEST_VAULT_PRIV, Network::Regtest).unwrap();

        // Fake prior funding UTXO.
        let prev_txid = Txid::from_raw_hash(sha256d::Hash::all_zeros());
        let utxo = VaultUtxo {
            txid: prev_txid,
            vout: 0,
            value_sat: 1_000_000,
        };
        let providers: Vec<Payout> = (0..2)
            .map(|i| {
                // Derive deterministic provider keys for the test.
                let mut seed = [0u8; 32];
                seed[31] = 1 + i as u8;
                let p_keys = BtcKeys::from_bytes(&seed, Network::Regtest).unwrap();
                Payout {
                    address: p_keys.p2wpkh_address(),
                    amount_sat: 50_000,
                }
            })
            .collect();
        let change_addr = keys.p2wpkh_address();
        let commit_hash = [0xABu8; 32];

        let tx = build_settlement_tx(&commit_hash, &providers, &[utxo], &change_addr, 500, &keys)
            .expect("build tx");

        // outputs = [op_return, payout_0, payout_1, change]
        assert_eq!(tx.output.len(), 4);
        assert!(tx.output[0].script_pubkey.is_op_return());
        assert_eq!(tx.output[1].value.to_sat(), 50_000);
        assert_eq!(tx.output[2].value.to_sat(), 50_000);
        assert_eq!(tx.output[3].value.to_sat(), 1_000_000 - 100_000 - 500);
        // witness present + populated for the input
        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.input[0].witness.len(), 2);
    }

    #[test]
    fn build_settlement_tx_p2tr_round_trip_offline() {
        // Verifies the BIP-341 key-path builder produces a well-formed tx
        // with a 64-byte Schnorr witness per input. End-to-end broadcast
        // is exercised by `btc_chain::tests::regtest_send_settlement_p2tr`.
        let keys = BtcKeys::from_hex(TEST_VAULT_PRIV, Network::Regtest).unwrap();

        let prev_txid = Txid::from_raw_hash(sha256d::Hash::all_zeros());
        let utxo = VaultUtxo {
            txid: prev_txid,
            vout: 0,
            value_sat: 1_000_000,
        };
        let providers: Vec<Payout> = (0..2)
            .map(|i| {
                let mut seed = [0u8; 32];
                seed[31] = 0x40 + i as u8;
                let p_keys = BtcKeys::from_bytes(&seed, Network::Regtest).unwrap();
                Payout {
                    // Mix output kinds: provider 0 = P2TR, provider 1 = P2WPKH.
                    address: if i == 0 {
                        p_keys.p2tr_address()
                    } else {
                        p_keys.p2wpkh_address()
                    },
                    amount_sat: 50_000,
                }
            })
            .collect();
        let change_addr = keys.p2tr_address();
        let commit_hash = [0xCDu8; 32];

        let tx = build_settlement_tx_p2tr(
            &commit_hash,
            &providers,
            &[utxo],
            &change_addr,
            500,
            &keys,
        )
        .expect("build p2tr tx");

        assert_eq!(tx.output.len(), 4);
        assert!(tx.output[0].script_pubkey.is_op_return());
        assert_eq!(tx.output[1].value.to_sat(), 50_000);
        assert_eq!(tx.output[2].value.to_sat(), 50_000);
        assert_eq!(tx.output[3].value.to_sat(), 1_000_000 - 100_000 - 500);
        // Taproot key-path witness: exactly one 64-byte Schnorr signature.
        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.input[0].witness.len(), 1);
        assert_eq!(tx.input[0].witness.iter().next().unwrap().len(), 64);
    }

    #[test]
    fn p2tr_address_differs_from_p2wpkh() {
        let keys = BtcKeys::from_hex(TEST_VAULT_PRIV, Network::Regtest).unwrap();
        let wpkh = keys.p2wpkh_address().to_string();
        let tr = keys.p2tr_address().to_string();
        assert_ne!(wpkh, tr);
        // P2TR addresses start with `bcrt1p` on regtest; P2WPKH with `bcrt1q`.
        assert!(tr.starts_with("bcrt1p"), "got: {tr}");
        assert!(wpkh.starts_with("bcrt1q"), "got: {wpkh}");
    }

    #[test]
    fn rejects_overpayment() {
        let keys = BtcKeys::from_hex(TEST_VAULT_PRIV, Network::Regtest).unwrap();
        let prev_txid = Txid::from_raw_hash(sha256d::Hash::all_zeros());
        let utxo = VaultUtxo {
            txid: prev_txid,
            vout: 0,
            value_sat: 1_000,
        };
        let payout = Payout {
            address: keys.p2wpkh_address(),
            amount_sat: 2_000,
        };
        let err =
            build_settlement_tx(&[0u8; 32], &[payout], &[utxo], &keys.p2wpkh_address(), 100, &keys)
                .unwrap_err();
        assert!(matches!(err, BtcError::InsufficientFunds { .. }));
    }
}
