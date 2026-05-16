//! openraft `TypeConfig` for the A402 committee.

use std::fmt::Display;
use std::io::Cursor;

use openraft::BasicNode;
use serde::{Deserialize, Serialize};

pub type NodeId = u64;

/// The payload Raft replicates. Slice 1 keeps it opaque (`Vec<u8>`) so we
/// don't couple to the enclave's [`a402-enclave::wal::WalEvent`] yet. The
/// integration test serializes its own struct into bytes for proposing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalEvent(pub Vec<u8>);

impl WalEvent {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl Display for WalEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WalEvent({} bytes)", self.0.len())
    }
}

openraft::declare_raft_types!(
    pub TypeConfig:
        D            = WalEvent,
        R            = (),
        NodeId       = NodeId,
        Node         = BasicNode,
        Entry        = openraft::Entry<TypeConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = openraft::TokioRuntime,
);
