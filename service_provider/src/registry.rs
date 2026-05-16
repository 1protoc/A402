//! `Reg_U` — the Service Provider's local registry of Vault TEEs it has
//! verified the attestation of (, lines 2–5).
//!
//! Each entry pins `(uid, pk_U, h_code,U)`. After registration the SP refuses
//! to talk to any Vault that doesn't appear here. This is the SP-side
//! mirror of `enclave/src/state.rs::ProviderRegistration` (which is
//! the Vault's `Reg_S`).
//!
//! For the initial refactor we only verify the on-chain `vault_eoa` matches
//! what the contract's `ASCManager.vault` returns — a degenerate form of
//! attestation gating that's still meaningful: it pins the on-chain
//! identity of the Vault we're willing to serve channels for. A future
//! commit upgrades this to verifying a full Nitro/SEV-SNP attestation
//! document.

use std::sync::RwLock;

use a402_shared::evm_chain::Address;
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct VaultRecord {
    pub uid: String,
    /// On-chain Vault EOA (the address `ASCManager` recognises as `vault`).
    pub vault_eoa: Address,
    /// Optional code-hash measurement from the Vault's attestation document.
    /// Empty for the initial demo path; populated once full attestation is
    /// wired in.
    pub code_hash: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
pub struct VaultRegistry {
    /// uid → record. We only keep one entry per uid; re-registering a
    /// different `vault_eoa` for the same uid is rejected as a safety net
    /// against accidental key rotation without a fresh uid.
    by_uid: DashMap<String, VaultRecord>,
    /// Lock used only when we need to enumerate or perform consistency
    /// checks across all entries; per-record lookups go through `DashMap`
    /// directly.
    _lock: RwLock<()>,
}

impl VaultRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the existing record (if any), or inserts the new one. On
    /// conflict — same `uid` already exists with a different `vault_eoa` —
    /// returns `Err` so the caller can surface a 409.
    pub fn register(&self, record: VaultRecord) -> Result<VaultRecord, String> {
        if let Some(existing) = self.by_uid.get(&record.uid) {
            if existing.vault_eoa != record.vault_eoa {
                return Err(format!(
                    "uid {} already pinned to vault_eoa {}, refusing to overwrite",
                    record.uid,
                    existing.vault_eoa.to_hex()
                ));
            }
            return Ok(existing.clone());
        }
        self.by_uid.insert(record.uid.clone(), record.clone());
        Ok(record)
    }

    pub fn get(&self, uid: &str) -> Option<VaultRecord> {
        self.by_uid.get(uid).map(|entry| entry.clone())
    }

    pub fn find_by_eoa(&self, eoa: &Address) -> Option<VaultRecord> {
        self.by_uid
            .iter()
            .find(|entry| &entry.value().vault_eoa == eoa)
            .map(|entry| entry.value().clone())
    }

    pub fn len(&self) -> usize {
        self.by_uid.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn register_round_trip() {
        let reg = VaultRegistry::new();
        let rec = VaultRecord {
            uid: "vault-1".to_string(),
            vault_eoa: addr(1),
            code_hash: None,
        };
        assert!(reg.register(rec.clone()).is_ok());
        let got = reg.get("vault-1").unwrap();
        assert_eq!(got.vault_eoa, rec.vault_eoa);
    }

    #[test]
    fn register_rejects_eoa_change_for_same_uid() {
        let reg = VaultRegistry::new();
        reg.register(VaultRecord {
            uid: "vault-1".to_string(),
            vault_eoa: addr(1),
            code_hash: None,
        })
        .unwrap();
        let err = reg
            .register(VaultRecord {
                uid: "vault-1".to_string(),
                vault_eoa: addr(2),
                code_hash: None,
            })
            .unwrap_err();
        assert!(err.contains("pinned"));
    }

    #[test]
    fn find_by_eoa_returns_matching_record() {
        let reg = VaultRegistry::new();
        reg.register(VaultRecord {
            uid: "vault-1".to_string(),
            vault_eoa: addr(7),
            code_hash: None,
        })
        .unwrap();
        assert!(reg.find_by_eoa(&addr(7)).is_some());
        assert!(reg.find_by_eoa(&addr(8)).is_none());
    }
}
