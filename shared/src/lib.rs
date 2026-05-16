//! Shared modules used by both the A402 Vault (`enclave/`) and the
//! Service Provider (`service_provider/`) Rust binaries.
//!
//! Per , Vault and Service Provider are independent TEEs with
//! independent keypairs and attestations — they only share PROTOCOL code
//! (Schnorr math, ABI encoders, EVM RPC). This crate is exactly that shared
//! protocol code; per-role keys / attestation / HTTP surfaces live in each
//! binary's own crate.

pub mod adaptor_sig_secp;
pub mod btc_asc_channel;
pub mod btc_asc_script;
pub mod btc_chain;
pub mod btc_tx;
pub mod evm_chain;
pub mod evm_channel_store;
pub mod evm_tx;

/// Re-export of the [`bitcoin`] crate so downstream consumers (e.g.
/// [`a402-enclave`]) can refer to `Address`, `Network`, `Txid`, etc.
/// without a direct dep on `bitcoin`.
pub use bitcoin;
