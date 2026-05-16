//! Vault-owned Bitcoin UTXO ledger (slice 3A).
//!
//! In production the Vault must not trust bitcoind's wallet to enumerate
//! its own funds — bitcoind sits outside the TEE and a compromised host
//! could omit / forge UTXOs. So the Vault tracks every UTXO it owns in
//! its own encrypted WAL:
//!
//!   - `BtcUtxoAdded`     — deposit observed at vault address (slice 3B
//!                          deposit detector writes this)
//!   - `BtcUtxoSpent`     — input consumed by a settlement batch
//!   - `BtcChangeCreated` — change output the same batch produced
//!
//! On startup the WAL is replayed and the in-memory ledger reconstructed.
//! The submitter (slice 3C) picks UTXOs from this ledger instead of
//! calling `listunspent`.
//!
//! This module is intentionally chain-state-only: no I/O, no signing.
//! `BtcUtxoLedger` is small enough to fit in `AppState` directly.

use std::collections::BTreeMap;

use a402_shared::bitcoin::Txid;
use a402_shared::btc_tx::VaultUtxo;
use thiserror::Error;

/// Key into the ledger: `(funding_txid, vout)` uniquely identifies a UTXO.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtxoKey {
    pub txid: Txid,
    pub vout: u32,
}

impl UtxoKey {
    pub fn new(txid: Txid, vout: u32) -> Self {
        Self { txid, vout }
    }

    pub fn from_utxo(u: &VaultUtxo) -> Self {
        Self {
            txid: u.txid,
            vout: u.vout,
        }
    }
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("UTXO {0:?}:{1} already in ledger")]
    Duplicate(Txid, u32),
    #[error("UTXO {0:?}:{1} not in ledger")]
    NotFound(Txid, u32),
    #[error("no single UTXO covers {needed} sats (largest available = {best_value} sats)")]
    InsufficientFunds { needed: u64, best_value: u64 },
}

#[derive(Debug, Default, Clone)]
pub struct BtcUtxoLedger {
    /// All currently unspent UTXOs the Vault knows it owns.
    /// Keyed by `(txid, vout)`, value = satoshis.
    utxos: BTreeMap<UtxoKey, u64>,
    /// Count of spends ever applied. Useful for replay sanity / metrics.
    spent_count: u64,
}

impl BtcUtxoLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply `BtcUtxoAdded` / `BtcChangeCreated`. Both are inserts; we
    /// reject duplicates so replay catches double-detection bugs.
    pub fn add(&mut self, key: UtxoKey, value_sat: u64) -> Result<(), LedgerError> {
        if self.utxos.contains_key(&key) {
            return Err(LedgerError::Duplicate(key.txid, key.vout));
        }
        self.utxos.insert(key, value_sat);
        Ok(())
    }

    /// Apply `BtcUtxoSpent`. Errors when the UTXO isn't present (replay-
    /// of-order bug or honest enclave restart with stale snapshot).
    pub fn spend(&mut self, key: &UtxoKey) -> Result<u64, LedgerError> {
        let value = self
            .utxos
            .remove(key)
            .ok_or(LedgerError::NotFound(key.txid, key.vout))?;
        self.spent_count += 1;
        Ok(value)
    }

    pub fn len(&self) -> usize {
        self.utxos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.utxos.is_empty()
    }

    pub fn total_sat(&self) -> u64 {
        self.utxos.values().sum()
    }

    pub fn spent_count(&self) -> u64 {
        self.spent_count
    }

    /// Snapshot the ledger as a sorted `Vec<VaultUtxo>`. Caller can pass
    /// this straight into [`a402_shared::btc_tx::build_settlement_tx`].
    pub fn snapshot(&self) -> Vec<VaultUtxo> {
        self.utxos
            .iter()
            .map(|(k, v)| VaultUtxo {
                txid: k.txid,
                vout: k.vout,
                value_sat: *v,
            })
            .collect()
    }

    /// Pick the smallest single UTXO covering `needed`. Multi-UTXO
    /// selection lands in a later slice.
    pub fn pick_single(&self, needed: u64) -> Result<VaultUtxo, LedgerError> {
        let mut best: Option<(&UtxoKey, &u64)> = None;
        let mut largest: u64 = 0;
        for (k, v) in &self.utxos {
            largest = largest.max(*v);
            if *v >= needed {
                match best {
                    None => best = Some((k, v)),
                    Some((_, prev)) if v < prev => best = Some((k, v)),
                    _ => {}
                }
            }
        }
        match best {
            Some((k, v)) => Ok(VaultUtxo {
                txid: k.txid,
                vout: k.vout,
                value_sat: *v,
            }),
            None => Err(LedgerError::InsufficientFunds {
                needed,
                best_value: largest,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a402_shared::bitcoin::hashes::{sha256d, Hash as _};

    fn txid(seed: u8) -> Txid {
        let mut bytes = [0u8; 32];
        bytes[31] = seed;
        Txid::from_raw_hash(sha256d::Hash::from_byte_array(bytes))
    }

    #[test]
    fn add_spend_snapshot_round_trip() {
        let mut l = BtcUtxoLedger::new();
        assert!(l.is_empty());

        l.add(UtxoKey::new(txid(1), 0), 100_000).unwrap();
        l.add(UtxoKey::new(txid(2), 0), 50_000).unwrap();
        l.add(UtxoKey::new(txid(3), 0), 200_000).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l.total_sat(), 350_000);

        let snap = l.snapshot();
        assert_eq!(snap.len(), 3);
        // BTreeMap iteration is sorted by key, so snapshot order is
        // deterministic — tests downstream can rely on it.
        assert_eq!(snap[0].value_sat, 100_000);
        assert_eq!(snap[1].value_sat, 50_000);
        assert_eq!(snap[2].value_sat, 200_000);

        let value = l.spend(&UtxoKey::new(txid(2), 0)).unwrap();
        assert_eq!(value, 50_000);
        assert_eq!(l.len(), 2);
        assert_eq!(l.spent_count(), 1);

        // Double-spend error
        let err = l.spend(&UtxoKey::new(txid(2), 0)).unwrap_err();
        assert!(matches!(err, LedgerError::NotFound(_, 0)));
    }

    #[test]
    fn rejects_duplicate_add() {
        let mut l = BtcUtxoLedger::new();
        l.add(UtxoKey::new(txid(1), 0), 100).unwrap();
        let err = l.add(UtxoKey::new(txid(1), 0), 200).unwrap_err();
        assert!(matches!(err, LedgerError::Duplicate(_, 0)));
    }

    #[test]
    fn pick_single_returns_smallest_covering() {
        let mut l = BtcUtxoLedger::new();
        l.add(UtxoKey::new(txid(1), 0), 50_000).unwrap();
        l.add(UtxoKey::new(txid(2), 0), 100_000).unwrap();
        l.add(UtxoKey::new(txid(3), 0), 200_000).unwrap();

        // Need 75_000 → smallest covering is 100_000 (txid 2).
        let picked = l.pick_single(75_000).unwrap();
        assert_eq!(picked.value_sat, 100_000);
        assert_eq!(picked.txid, txid(2));

        // Need 300_000 → no single UTXO covers; error reports largest.
        let err = l.pick_single(300_000).unwrap_err();
        assert!(matches!(
            err,
            LedgerError::InsufficientFunds {
                needed: 300_000,
                best_value: 200_000
            }
        ));
    }
}
