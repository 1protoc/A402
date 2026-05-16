//! Bitcoin ASC Taproot script tree (slice 4B).
//!
//! Maps the EVM ASC manager's three resolution paths onto a Taproot
//! output. Cooperative close stays in the **key-path** so on-chain
//! observers can't tell an ASC channel apart from a vanilla single-sig
//! P2TR transfer. The two adversarial recovery paths sit in the
//! **script-path** tap-leaves.
//!
//! ```text
//!                            P2TR output
//!                            (internal key Q = K_coop tweaked by H_TapTweak(K_coop || merkle_root))
//!                                 │
//!         ┌───────────────────────┴───────────────────────┐
//!         │ key-path spend                                │ script-path spend
//!         │ (cooperative close, all parties signed off)   │ (adversarial recovery)
//!         │ → 1 × Schnorr sig under Q, witness = [sig]    │
//!         │ → on-chain looks identical to a wallet xfer   │
//!         ▼                                                ▼
//!     committed off-chain (Client + Vault committee + SP)        ┌────────────────────────────────────┐
//!                                                                │ tap-leaf A: "force-close after T"  │
//!                                                                │   <T> OP_CSV OP_DROP <vault_xonly> │
//!                                                                │   OP_CHECKSIG                      │
//!                                                                ├────────────────────────────────────┤
//!                                                                │ tap-leaf B: "adv-vault recovery"   │
//!                                                                │   <client_xonly> OP_CHECKSIGVERIFY │
//!                                                                │   <sp_xonly> OP_CHECKSIG           │
//!                                                                └────────────────────────────────────┘
//! ```
//!
//! Slice 4B (this module) defines the data + builds the address.
//! Slice 4C will add the off-chain state machine + signing helpers
//! (`cooperative_close_sig`, `force_close_with_csv`, `adv_vault_recover`).

use bitcoin::address::Address;
use bitcoin::blockdata::opcodes::all::{OP_CHECKSIG, OP_CHECKSIGVERIFY, OP_CSV, OP_DROP};
use bitcoin::key::XOnlyPublicKey;
use bitcoin::script::Builder;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::taproot::{LeafVersion, TaprootBuilder, TaprootBuilderError, TaprootSpendInfo};
use bitcoin::{Network, ScriptBuf};
use thiserror::Error;

/// Configuration for an A402 Bitcoin ASC channel's Taproot output.
///
/// All fields are deterministic: two channels with the same `AscScriptTree`
/// values produce byte-identical scripts + the same Taproot output key.
#[derive(Debug, Clone)]
pub struct AscScriptTree {
    /// Internal key used for the cooperative key-path spend. In production
    /// this is a MuSig2 / FROST aggregate of (Client, Vault, SP);
    /// for development we accept any single key, which still proves the
    /// surrounding ASC machinery works.
    pub cooperative_xonly: XOnlyPublicKey,
    /// Vault's solo x-only key, used by the timeout force-close leaf.
    pub vault_xonly: XOnlyPublicKey,
    /// Client's x-only key, used by the adv-vault recovery leaf.
    pub client_xonly: XOnlyPublicKey,
    /// Service Provider's x-only key, used by the adv-vault recovery leaf.
    pub sp_xonly: XOnlyPublicKey,
    /// CSV relative-locktime in blocks (matches ASCManager.sol's dispute
    /// window). Anyone holding `vault_xonly` can spend via leaf A only
    /// after this many blocks have elapsed since the funding tx confirmed.
    pub dispute_window_blocks: u32,
}

#[derive(Debug, Error)]
pub enum AscScriptError {
    #[error("taproot builder: {0}")]
    Builder(String),
    #[error("dispute_window_blocks must be > 0 and ≤ 65535 (CSV 16-bit relative-time limit)")]
    InvalidWindow,
}

impl From<TaprootBuilderError> for AscScriptError {
    fn from(e: TaprootBuilderError) -> Self {
        AscScriptError::Builder(e.to_string())
    }
}

/// Output of [`AscScriptTree::build`]: the deposit address an on-chain
/// `createASC` would pay into, plus the full `TaprootSpendInfo` needed by
/// slice 4C to construct script-path control blocks.
#[derive(Debug, Clone)]
pub struct AscTaproot {
    pub address: Address,
    pub spend_info: TaprootSpendInfo,
    pub force_close_script: ScriptBuf,
    pub adv_vault_script: ScriptBuf,
}

impl AscScriptTree {
    /// Compile to a Taproot output. Mirrors EVM `createASC(cid, …)` in the
    /// sense that every channel has exactly one such address.
    pub fn build(&self, network: Network) -> Result<AscTaproot, AscScriptError> {
        if self.dispute_window_blocks == 0 || self.dispute_window_blocks > u16::MAX as u32 {
            return Err(AscScriptError::InvalidWindow);
        }

        let force_close = force_close_script(self.vault_xonly, self.dispute_window_blocks);
        let adv_vault = adv_vault_recovery_script(self.client_xonly, self.sp_xonly);

        // Two-leaf balanced tree: both branches at depth 1, so merkle root
        // = TapBranchHash(sort(leaf_A, leaf_B)).
        let secp = Secp256k1::verification_only();
        let spend_info = TaprootBuilder::new()
            .add_leaf(1, force_close.clone())?
            .add_leaf(1, adv_vault.clone())?
            .finalize(&secp, self.cooperative_xonly)
            .map_err(|_| {
                AscScriptError::Builder(
                    "TaprootBuilder::finalize returned an incomplete tree".to_string(),
                )
            })?;

        let address = Address::p2tr(
            &secp,
            self.cooperative_xonly,
            spend_info.merkle_root(),
            network,
        );

        Ok(AscTaproot {
            address,
            spend_info,
            force_close_script: force_close,
            adv_vault_script: adv_vault,
        })
    }
}

/// Force-close after CSV delay ( adversarial-client recovery).
///
/// `<delay> OP_CSV OP_DROP <vault_xonly> OP_CHECKSIG`
pub fn force_close_script(vault_xonly: XOnlyPublicKey, delay_blocks: u32) -> ScriptBuf {
    Builder::new()
        .push_int(delay_blocks as i64)
        .push_opcode(OP_CSV)
        .push_opcode(OP_DROP)
        .push_x_only_key(&vault_xonly)
        .push_opcode(OP_CHECKSIG)
        .into_script()
}

/// Adversarial-vault recovery: Client + SP sign jointly (,
/// matches EVM `initForceClose` + `finalForceClose` collapsed into a
/// single on-chain script that bypasses the Vault entirely).
///
/// `<client_xonly> OP_CHECKSIGVERIFY <sp_xonly> OP_CHECKSIG`
pub fn adv_vault_recovery_script(
    client_xonly: XOnlyPublicKey,
    sp_xonly: XOnlyPublicKey,
) -> ScriptBuf {
    Builder::new()
        .push_x_only_key(&client_xonly)
        .push_opcode(OP_CHECKSIGVERIFY)
        .push_x_only_key(&sp_xonly)
        .push_opcode(OP_CHECKSIG)
        .into_script()
}

/// Helper for tests + slice 4C: produce the `(script, leaf_version)` pair
/// that script-path control blocks expect.
pub fn leaf_for(script: &ScriptBuf) -> (ScriptBuf, LeafVersion) {
    (script.clone(), LeafVersion::TapScript)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::btc_tx::BtcKeys;

    fn xonly(seed_byte: u8) -> XOnlyPublicKey {
        let mut seed = [0u8; 32];
        seed[31] = seed_byte;
        BtcKeys::from_bytes(&seed, Network::Regtest)
            .unwrap()
            .taproot_internal_xonly()
    }

    #[test]
    fn build_produces_bcrt1p_address() {
        let tree = AscScriptTree {
            cooperative_xonly: xonly(0x01),
            vault_xonly: xonly(0x02),
            client_xonly: xonly(0x03),
            sp_xonly: xonly(0x04),
            dispute_window_blocks: 144,
        };
        let asc = tree.build(Network::Regtest).expect("build");
        let addr = asc.address.to_string();
        assert!(addr.starts_with("bcrt1p"), "got {addr}");
    }

    #[test]
    fn force_close_script_shape() {
        let s = force_close_script(xonly(0x02), 144);
        // First push is the CSV delay (small int → single OP_PUSHNUM_x or
        // pushdata depending on value); next byte must be OP_CSV (0xb2).
        let bytes = s.as_bytes();
        assert!(
            bytes.contains(&0xb2),
            "force-close script must contain OP_CSV: {bytes:?}"
        );
        assert!(
            bytes.contains(&0x75),
            "force-close script must contain OP_DROP: {bytes:?}"
        );
        assert!(
            bytes.contains(&0xac),
            "force-close script must end with OP_CHECKSIG: {bytes:?}"
        );
    }

    #[test]
    fn adv_vault_script_shape() {
        let s = adv_vault_recovery_script(xonly(0x03), xonly(0x04));
        let bytes = s.as_bytes();
        // Must contain OP_CHECKSIGVERIFY (0xad) and OP_CHECKSIG (0xac).
        assert!(bytes.contains(&0xad), "must contain OP_CHECKSIGVERIFY");
        assert!(bytes.contains(&0xac), "must contain OP_CHECKSIG");
        // Two 32-byte x-only key pushes → two `0x20` length prefixes.
        let push20_count = bytes.iter().filter(|b| **b == 0x20).count();
        assert!(push20_count >= 2, "expected ≥2 32-byte pushes, got {push20_count}");
    }

    #[test]
    fn build_is_deterministic() {
        // Two channels with identical config produce identical addresses
        // (and identical merkle roots / output keys).
        let make = || AscScriptTree {
            cooperative_xonly: xonly(0x01),
            vault_xonly: xonly(0x02),
            client_xonly: xonly(0x03),
            sp_xonly: xonly(0x04),
            dispute_window_blocks: 144,
        };
        let a = make().build(Network::Regtest).unwrap();
        let b = make().build(Network::Regtest).unwrap();
        assert_eq!(a.address.to_string(), b.address.to_string());
        assert_eq!(a.spend_info.merkle_root(), b.spend_info.merkle_root());
    }

    #[test]
    fn changing_dispute_window_changes_address() {
        let make = |window| AscScriptTree {
            cooperative_xonly: xonly(0x01),
            vault_xonly: xonly(0x02),
            client_xonly: xonly(0x03),
            sp_xonly: xonly(0x04),
            dispute_window_blocks: window,
        };
        let a = make(144).build(Network::Regtest).unwrap();
        let b = make(288).build(Network::Regtest).unwrap();
        assert_ne!(a.address.to_string(), b.address.to_string());
    }

    #[test]
    fn rejects_invalid_window() {
        let tree = AscScriptTree {
            cooperative_xonly: xonly(0x01),
            vault_xonly: xonly(0x02),
            client_xonly: xonly(0x03),
            sp_xonly: xonly(0x04),
            dispute_window_blocks: 0,
        };
        assert!(matches!(
            tree.build(Network::Regtest),
            Err(AscScriptError::InvalidWindow)
        ));
    }
}
