//! In-memory state for the EVM ASC channels the Rust enclave manages.
//!
//! Lives alongside the on-chain `ASCManager` contract — this is the
//! enclave-side mirror that tracks per-channel balance, the latest
//! cooperatively-signed state, and the per-request adaptor signature material
//! that lets us complete `closeASC` or `forceClose` later.
//!
//! Scope intentionally small: no persistence, no WAL. The full enclave WAL
//! already covers the Solana ASC path; the EVM equivalent will come once the
//! HTTP surface stabilises. For now the demo expectation is: the enclave is
//! either online and serving, or it gets restarted and the on-chain channels
//! can still be closed by the buyer's `initForceClose` recovery path.
//!
//! Concurrency: one mutex per channel. We use a global `DashMap<Bytes32, ...>`
//! so handlers never race on the same channel state. The store is a
//! `OnceLock<EvmChannelStore>` populated on first use.

use std::sync::OnceLock;

use dashmap::DashMap;
use k256::Scalar;

use crate::adaptor_sig_secp::{AdaptorPreSignature, EncryptedResult};
use crate::evm_chain::{Address, Bytes32};

/// Persistent (in-memory) state for a single EVM ASC channel.
#[derive(Debug, Clone)]
pub struct EvmChannelRecord {
    pub buyer: Address,
    pub seller: Address,
    pub total_deposit: u128,
    pub balance_c: u128,
    pub balance_s: u128,
    pub version: u64,
    /// Last cooperatively-signed buyer ECDSA over `ascStateHash`.
    pub last_sig_c: Option<Vec<u8>>,
    /// Last cooperatively-signed seller ECDSA over `ascStateHash`.
    pub last_sig_s: Option<Vec<u8>>,
    /// The Schnorr adaptor pre-signature corresponding to the last accepted
    /// request — kept so the seller can produce a complete `σ_S` for an
    /// on-chain `forceClose` without re-requesting from the buyer.
    pub last_sig_hat: Option<AdaptorPreSignature>,
    /// The witness `t` we revealed at the last `/finalize`. Stored alongside
    /// `last_sig_hat` so `forceClose` can be produced unilaterally.
    pub last_t: Option<Scalar>,
    pub last_state_hash: Option<Bytes32>,
    pub served_requests: u32,
    pub force_closed: bool,
}

impl EvmChannelRecord {
    pub fn new(buyer: Address, seller: Address, total_deposit: u128) -> Self {
        Self {
            buyer,
            seller,
            total_deposit,
            balance_c: total_deposit,
            balance_s: 0,
            version: 0,
            last_sig_c: None,
            last_sig_s: None,
            last_sig_hat: None,
            last_t: None,
            last_state_hash: None,
            served_requests: 0,
            force_closed: false,
        }
    }
}

/// A phase-1 request waiting for the buyer's σ_C at `/finalize`. Holds the
/// secret witness `t` until the buyer authorises payment.
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub cid: Bytes32,
    pub new_version: u64,
    pub new_balance_c: u128,
    pub new_balance_s: u128,
    pub t: Scalar,
    pub sig_hat: AdaptorPreSignature,
    pub enc_res: EncryptedResult,
    pub asc_state_hash: Bytes32,
}

#[derive(Debug, Default)]
pub struct EvmChannelStore {
    channels: DashMap<[u8; 32], EvmChannelRecord>,
    /// Pending requests keyed by `(cid, new_version)`. A finalize call must
    /// pass the exact version it expects to advance to.
    pending: DashMap<(Bytes32, u64), PendingRequest>,
}

impl EvmChannelStore {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
            pending: DashMap::new(),
        }
    }

    pub fn insert_channel(&self, cid: &Bytes32, record: EvmChannelRecord) {
        self.channels.insert(cid.0, record);
    }

    pub fn get_channel(&self, cid: &Bytes32) -> Option<EvmChannelRecord> {
        self.channels.get(&cid.0).map(|entry| entry.clone())
    }

    /// Apply a mutation under the channel's per-key lock. Returns the
    /// closure's result.
    pub fn mutate_channel<F, R>(&self, cid: &Bytes32, f: F) -> Option<R>
    where
        F: FnOnce(&mut EvmChannelRecord) -> R,
    {
        let mut entry = self.channels.get_mut(&cid.0)?;
        Some(f(&mut entry))
    }

    pub fn park_pending(&self, request: PendingRequest) {
        let key = (request.cid, request.new_version);
        self.pending.insert(key, request);
    }

    pub fn take_pending(&self, cid: &Bytes32, version: u64) -> Option<PendingRequest> {
        self.pending.remove(&(*cid, version)).map(|(_k, v)| v)
    }
}

/// Process-global EVM channel store. Lazily initialised on first access.
pub fn store() -> &'static EvmChannelStore {
    static STORE: OnceLock<EvmChannelStore> = OnceLock::new();
    STORE.get_or_init(EvmChannelStore::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_addr(byte: u8) -> Address {
        Address([byte; 20])
    }

    #[test]
    fn channel_round_trip() {
        let store = EvmChannelStore::new();
        let cid = Bytes32([1u8; 32]);
        store.insert_channel(&cid, EvmChannelRecord::new(dummy_addr(2), dummy_addr(3), 1000));
        let r = store.get_channel(&cid).unwrap();
        assert_eq!(r.total_deposit, 1000);
        assert_eq!(r.balance_c, 1000);
        assert_eq!(r.balance_s, 0);
        assert_eq!(r.version, 0);
    }

    #[test]
    fn mutate_channel_advances_version() {
        let store = EvmChannelStore::new();
        let cid = Bytes32([2u8; 32]);
        store.insert_channel(&cid, EvmChannelRecord::new(dummy_addr(2), dummy_addr(3), 1000));
        store.mutate_channel(&cid, |r| {
            r.version = 7;
            r.balance_c = 900;
            r.balance_s = 100;
        });
        let r = store.get_channel(&cid).unwrap();
        assert_eq!(r.version, 7);
        assert_eq!(r.balance_c, 900);
        assert_eq!(r.balance_s, 100);
    }
}
