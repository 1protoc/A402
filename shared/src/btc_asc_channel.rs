//! Bitcoin ASC channel signing helpers (slice 4C).
//!
//! Builds and signs the three spend paths exposed by
//! [`crate::btc_asc_script::AscScriptTree`]:
//!
//!   - `cooperative_close_tx`  — Taproot key-path spend (1 × Schnorr sig
//!                                under the tap-tweaked aggregate key);
//!                                indistinguishable from a wallet xfer.
//!   - `force_close_csv_tx`    — script-path spend of leaf A
//!                                (`<T> CSV DROP <vault> CHECKSIG`);
//!                                only valid after `dispute_window_blocks`
//!                                have elapsed on the input's BIP-68
//!                                relative locktime.
//!   - `adv_vault_recovery_tx` — script-path spend of leaf B
//!                                (`<client> CHECKSIGVERIFY <sp> CHECKSIG`);
//!                                bypasses the Vault entirely.
//!
//! All three produce a [`bitcoin::Transaction`] with witnesses populated.
//! Broadcast via [`crate::btc_chain::BtcRpcClient::sendrawtransaction`].

use bitcoin::address::Address;
use bitcoin::hashes::Hash;
use bitcoin::key::{Keypair, TapTweak};
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::{LeafVersion, TapLeafHash};
use bitcoin::transaction::Version;
use bitcoin::{
    Amount, Network, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness,
};

use crate::btc_asc_script::{AscScriptTree, AscTaproot};
use crate::btc_tx::{BtcError, VaultUtxo};

/// One output of an ASC close — the final payment to a participant.
#[derive(Debug, Clone)]
pub struct ChannelOutput {
    pub address: Address,
    pub amount_sat: u64,
}

/// Per-channel state. Bundles the on-chain identity (funding UTXO + the
/// `AscScriptTree` that derived its address) with everything the signing
/// helpers need.
#[derive(Debug, Clone)]
pub struct BtcAscChannel {
    pub script_tree: AscScriptTree,
    pub network: Network,
    pub funding: VaultUtxo,
    pub taproot: AscTaproot,
}

impl BtcAscChannel {
    /// Build a channel from a fully resolved script tree + the funding
    /// UTXO that landed at its address. Caller is responsible for
    /// confirming on-chain that `funding.txid:vout` actually pays into
    /// `taproot.address` for `funding.value_sat`.
    pub fn new(
        script_tree: AscScriptTree,
        network: Network,
        funding: VaultUtxo,
    ) -> Result<Self, BtcError> {
        let taproot = script_tree
            .build(network)
            .map_err(|e| BtcError::Other(format!("ASC script tree build: {e}")))?;
        Ok(Self {
            script_tree,
            network,
            funding,
            taproot,
        })
    }

    /// Cooperative close: Schnorr key-path spend under the tap-tweaked
    /// cooperative key. `cooperative_kp` MUST be a keypair whose x-only
    /// pubkey matches `script_tree.cooperative_xonly` — in MVP that's a
    /// single signer; in production it's the MuSig2 aggregated key after
    /// rounds 1/2 are complete.
    pub fn cooperative_close_tx(
        &self,
        outputs: &[ChannelOutput],
        fee_sat: u64,
        cooperative_kp: &Keypair,
    ) -> Result<Transaction, BtcError> {
        if outputs.is_empty() {
            return Err(BtcError::NoPayouts);
        }
        let mut tx = scaffold_tx(&self.funding, outputs, fee_sat, Sequence::ENABLE_RBF_NO_LOCKTIME)?;
        let prevouts = self.prevouts();
        let secp = Secp256k1::new();

        // Tap-tweak with the channel's merkle root, then key-path sign.
        let tweaked = cooperative_kp
            .tap_tweak(&secp, self.taproot.spend_info.merkle_root())
            .to_inner();
        let sighash = SighashCache::new(&mut tx)
            .taproot_key_spend_signature_hash(
                0,
                &Prevouts::All(&prevouts),
                TapSighashType::Default,
            )
            .map_err(|e| BtcError::Sighash(format!("key-spend sighash: {e}")))?;
        let msg = Message::from_digest_slice(sighash.as_byte_array())
            .map_err(|e| BtcError::Sighash(format!("Message: {e}")))?;
        let sig = secp.sign_schnorr_no_aux_rand(&msg, &tweaked);

        let mut w = Witness::new();
        w.push(sig.as_ref());
        tx.input[0].witness = w;
        Ok(tx)
    }

    /// Force-close after CSV expiry: script-path spend of leaf A.
    /// `vault_kp` must match `script_tree.vault_xonly`. The input's
    /// `nSequence` is set to `dispute_window_blocks` so the transaction
    /// is only accepted once that many blocks have elapsed since the
    /// funding tx confirmed (BIP-68 + BIP-112).
    pub fn force_close_csv_tx(
        &self,
        outputs: &[ChannelOutput],
        fee_sat: u64,
        vault_kp: &Keypair,
    ) -> Result<Transaction, BtcError> {
        if outputs.is_empty() {
            return Err(BtcError::NoPayouts);
        }
        let delay = self.script_tree.dispute_window_blocks;
        if delay > u16::MAX as u32 {
            return Err(BtcError::Other("dispute window exceeds u16 CSV limit".into()));
        }
        let sequence = Sequence::from_height(delay as u16);
        let mut tx = scaffold_tx(&self.funding, outputs, fee_sat, sequence)?;
        let prevouts = self.prevouts();
        let leaf_hash = TapLeafHash::from_script(
            &self.taproot.force_close_script,
            LeafVersion::TapScript,
        );
        let sig = sign_script_leaf(
            &mut tx,
            0,
            &prevouts,
            &leaf_hash,
            vault_kp,
        )?;

        let control_block = self
            .taproot
            .spend_info
            .control_block(&(
                self.taproot.force_close_script.clone(),
                LeafVersion::TapScript,
            ))
            .ok_or_else(|| BtcError::Other("force-close leaf missing from spend_info".into()))?;
        let mut w = Witness::new();
        w.push(sig.as_ref());
        w.push(self.taproot.force_close_script.as_bytes());
        w.push(control_block.serialize());
        tx.input[0].witness = w;
        Ok(tx)
    }

    /// Adversarial-vault recovery: script-path spend of leaf B.
    /// `client_kp` matches `script_tree.client_xonly`, `sp_kp` matches
    /// `script_tree.sp_xonly`. No CSV delay applies.
    ///
    /// Witness order on the stack must put `client_sig` on top (because
    /// `CHECKSIGVERIFY` runs first, popping it). Push order is the
    /// reverse: `sp_sig` first, `client_sig` second, then the script and
    /// control block.
    pub fn adv_vault_recovery_tx(
        &self,
        outputs: &[ChannelOutput],
        fee_sat: u64,
        client_kp: &Keypair,
        sp_kp: &Keypair,
    ) -> Result<Transaction, BtcError> {
        if outputs.is_empty() {
            return Err(BtcError::NoPayouts);
        }
        let mut tx = scaffold_tx(&self.funding, outputs, fee_sat, Sequence::ENABLE_RBF_NO_LOCKTIME)?;
        let prevouts = self.prevouts();
        let leaf_hash = TapLeafHash::from_script(
            &self.taproot.adv_vault_script,
            LeafVersion::TapScript,
        );
        let client_sig = sign_script_leaf(
            &mut tx,
            0,
            &prevouts,
            &leaf_hash,
            client_kp,
        )?;
        let sp_sig = sign_script_leaf(
            &mut tx,
            0,
            &prevouts,
            &leaf_hash,
            sp_kp,
        )?;

        let control_block = self
            .taproot
            .spend_info
            .control_block(&(
                self.taproot.adv_vault_script.clone(),
                LeafVersion::TapScript,
            ))
            .ok_or_else(|| BtcError::Other("adv-vault leaf missing from spend_info".into()))?;
        let mut w = Witness::new();
        // Stack bottom → top order matches push order: sp_sig first (bottom),
        // client_sig second (top, popped first by CHECKSIGVERIFY).
        w.push(sp_sig.as_ref());
        w.push(client_sig.as_ref());
        w.push(self.taproot.adv_vault_script.as_bytes());
        w.push(control_block.serialize());
        tx.input[0].witness = w;
        Ok(tx)
    }

    fn prevouts(&self) -> Vec<TxOut> {
        vec![TxOut {
            value: Amount::from_sat(self.funding.value_sat),
            script_pubkey: self.taproot.address.script_pubkey(),
        }]
    }
}

/* -------------------------------------------------------------------------- */
/*                                  Helpers                                    */
/* -------------------------------------------------------------------------- */

fn scaffold_tx(
    funding: &VaultUtxo,
    outputs: &[ChannelOutput],
    fee_sat: u64,
    sequence: Sequence,
) -> Result<Transaction, BtcError> {
    let total_out: u64 = outputs.iter().map(|o| o.amount_sat).sum();
    if funding.value_sat < total_out + fee_sat {
        return Err(BtcError::InsufficientFunds {
            input_sat: funding.value_sat,
            output_sat: total_out,
            fee_sat,
        });
    }

    let txin = TxIn {
        previous_output: OutPoint {
            txid: funding.txid,
            vout: funding.vout,
        },
        script_sig: ScriptBuf::new(),
        sequence,
        witness: Witness::new(),
    };
    let txouts: Vec<TxOut> = outputs
        .iter()
        .map(|o| TxOut {
            value: Amount::from_sat(o.amount_sat),
            script_pubkey: o.address.script_pubkey(),
        })
        .collect();
    Ok(Transaction {
        version: Version::TWO,
        lock_time: bitcoin::absolute::LockTime::ZERO,
        input: vec![txin],
        output: txouts,
    })
}

fn sign_script_leaf(
    tx: &mut Transaction,
    input_idx: usize,
    prevouts: &[TxOut],
    leaf_hash: &TapLeafHash,
    kp: &Keypair,
) -> Result<bitcoin::secp256k1::schnorr::Signature, BtcError> {
    let secp = Secp256k1::new();
    let sighash = SighashCache::new(tx)
        .taproot_script_spend_signature_hash(
            input_idx,
            &Prevouts::All(prevouts),
            *leaf_hash,
            TapSighashType::Default,
        )
        .map_err(|e| BtcError::Sighash(format!("script-spend sighash: {e}")))?;
    let msg = Message::from_digest_slice(sighash.as_byte_array())
        .map_err(|e| BtcError::Sighash(format!("Message: {e}")))?;
    Ok(secp.sign_schnorr_no_aux_rand(&msg, kp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc_tx::BtcKeys;
    use bitcoin::hashes::sha256d;
    use bitcoin::Txid;

    fn keys(seed: u8) -> BtcKeys {
        let mut s = [0u8; 32];
        s[31] = seed;
        BtcKeys::from_bytes(&s, Network::Regtest).unwrap()
    }

    fn keypair(seed: u8) -> Keypair {
        let secp = Secp256k1::new();
        let mut s = [0u8; 32];
        s[31] = seed;
        let sk = bitcoin::secp256k1::SecretKey::from_slice(&s).unwrap();
        Keypair::from_secret_key(&secp, &sk)
    }

    fn channel() -> BtcAscChannel {
        let coop = keys(0x10);
        let vault = keys(0x20);
        let client = keys(0x30);
        let sp = keys(0x40);
        let tree = AscScriptTree {
            cooperative_xonly: coop.taproot_internal_xonly(),
            vault_xonly: vault.taproot_internal_xonly(),
            client_xonly: client.taproot_internal_xonly(),
            sp_xonly: sp.taproot_internal_xonly(),
            dispute_window_blocks: 144,
        };
        let funding = VaultUtxo {
            txid: Txid::from_raw_hash(sha256d::Hash::all_zeros()),
            vout: 0,
            value_sat: 100_000,
        };
        BtcAscChannel::new(tree, Network::Regtest, funding).unwrap()
    }

    fn payouts() -> Vec<ChannelOutput> {
        vec![
            ChannelOutput {
                address: keys(0x30).p2tr_address(),
                amount_sat: 60_000,
            },
            ChannelOutput {
                address: keys(0x40).p2tr_address(),
                amount_sat: 39_000,
            },
        ]
    }

    #[test]
    fn cooperative_close_witness_is_single_64b_sig() {
        let ch = channel();
        let coop_kp = keypair(0x10);
        let tx = ch
            .cooperative_close_tx(&payouts(), 1_000, &coop_kp)
            .expect("cooperative close");

        // Key-path spend → 1 witness item, 64 bytes.
        assert_eq!(tx.input[0].witness.len(), 1);
        assert_eq!(tx.input[0].witness.iter().next().unwrap().len(), 64);
        // Sequence should NOT be a CSV value.
        assert_eq!(tx.input[0].sequence, Sequence::ENABLE_RBF_NO_LOCKTIME);
    }

    #[test]
    fn force_close_witness_has_sig_script_control_block_and_csv_sequence() {
        let ch = channel();
        let vault_kp = keypair(0x20);
        let tx = ch
            .force_close_csv_tx(&payouts(), 1_000, &vault_kp)
            .expect("force-close");

        // Script-path spend → [sig, script, control_block] = 3 items.
        assert_eq!(tx.input[0].witness.len(), 3);
        let items: Vec<_> = tx.input[0].witness.iter().collect();
        assert_eq!(items[0].len(), 64, "vault Schnorr sig");
        // Sequence must satisfy CSV(144).
        assert_eq!(tx.input[0].sequence, Sequence::from_height(144));
    }

    #[test]
    fn adv_vault_witness_pushes_sp_then_client_then_script_then_control() {
        let ch = channel();
        let client_kp = keypair(0x30);
        let sp_kp = keypair(0x40);
        let tx = ch
            .adv_vault_recovery_tx(&payouts(), 1_000, &client_kp, &sp_kp)
            .expect("adv-vault recovery");

        // Witness = [sp_sig, client_sig, script, control_block].
        assert_eq!(tx.input[0].witness.len(), 4);
        let items: Vec<_> = tx.input[0].witness.iter().collect();
        assert_eq!(items[0].len(), 64, "sp Schnorr sig (stack bottom)");
        assert_eq!(items[1].len(), 64, "client Schnorr sig (stack top)");
        // sp_sig and client_sig must be different (verifying we didn't use
        // the same keypair for both).
        assert_ne!(items[0], items[1]);
    }

    #[test]
    fn cooperative_close_rejects_overpayment() {
        let ch = channel();
        let coop_kp = keypair(0x10);
        let too_big = vec![ChannelOutput {
            address: keys(0x30).p2tr_address(),
            amount_sat: 200_000,
        }];
        let err = ch
            .cooperative_close_tx(&too_big, 1_000, &coop_kp)
            .unwrap_err();
        assert!(matches!(err, BtcError::InsufficientFunds { .. }));
    }
}
