//! A402 Raft committee.
//!
//! Slice 1 (this file): an in-process 4-node Raft committee proving openraft
//! drives consensus over an opaque `WalEvent` payload type. Slice 2 will wire
//! this into [`enclave/`] so `/v1/verify` and `/v1/settle` propose through
//! Raft before returning 200.
//!
//! Design choices (per project README's "Implementation status" Phase Raft):
//!   - WAL-only replication: the Raft log carries A402 WAL events, the
//!     state machine applies them to the in-memory Vault state.
//!   - Single signing key (sk_U) replicated across all 4 TEEs in this slice;
//!     threshold ECDSA (FROST-secp256k1) is a follow-up phase.
//!   - mTLS / HTTP transport (this slice uses plain HTTP for the test;
//!     mTLS gating lands when wired into enclave).
//!   - Optional: a single-node deployment skips the committee entirely
//!     when `A402_RAFT_PEERS` is empty.

mod committee;
mod http_net;
mod network;
mod store;
mod types;

pub use committee::{CommitteeConfig, RaftCommittee, RaftError};
pub use http_net::{http_router, HttpFactory, HttpPeerMap};
pub use network::Router;
pub use types::{NodeId, TypeConfig, WalEvent};

// Re-export the openraft types that consumers of `a402-raft` need to construct
// configs / inspect metrics, so callers don't have to pin openraft directly.
pub use openraft::BasicNode;
