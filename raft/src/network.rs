//! In-process Raft RPC network for slice 1.
//!
//! The test bootstraps four peers in the same `tokio` runtime and shares a
//! [`Router`] between them: each peer's `RaftNetworkFactory` looks up its
//! target peer's [`Raft`] handle and calls `append_entries` / `vote` /
//! `install_snapshot` directly. There's no actual network I/O — but every
//! payload goes through `bincode` round-trips inside openraft, so the wire
//! shape stays honest.
//!
//! Slice 2 swaps this for an axum HTTP transport with mTLS between peers
//! (see [`crate::types::TypeConfig`] — the openraft TypeConfig is reused
//! verbatim).

use std::collections::HashMap;
use std::sync::Arc;

use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError};
use openraft::network::{RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{BasicNode, Raft};
use tokio::sync::RwLock;

use crate::types::{NodeId, TypeConfig};

/// Shared registry of in-process peers. Every node's [`InProcessFactory`]
/// holds an `Arc<Router>` and looks up its targets here at RPC time.
#[derive(Clone, Default)]
pub struct Router {
    inner: Arc<RwLock<HashMap<NodeId, Raft<TypeConfig>>>>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, node_id: NodeId, raft: Raft<TypeConfig>) {
        self.inner.write().await.insert(node_id, raft);
    }

    async fn get(&self, node_id: NodeId) -> Result<Raft<TypeConfig>, NetworkError> {
        self.inner.read().await.get(&node_id).cloned().ok_or_else(|| {
            NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("peer {node_id} not registered in Router"),
            ))
        })
    }
}

/// Factory that vends a [`PeerNetwork`] per target node.
#[derive(Clone)]
pub struct InProcessFactory {
    pub router: Router,
}

impl InProcessFactory {
    pub fn new(router: Router) -> Self {
        Self { router }
    }
}

impl RaftNetworkFactory<TypeConfig> for InProcessFactory {
    type Network = PeerNetwork;

    async fn new_client(&mut self, target: NodeId, _node: &BasicNode) -> Self::Network {
        PeerNetwork {
            target,
            router: self.router.clone(),
        }
    }
}

/// One-to-one network handle: every method just calls the corresponding
/// `Raft` method on the target node.
pub struct PeerNetwork {
    target: NodeId,
    router: Router,
}

impl RaftNetwork<TypeConfig> for PeerNetwork {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let target = self.router.get(self.target).await.map_err(RPCError::Network)?;
        target.append_entries(rpc).await.map_err(rpc_err)
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, BasicNode, RaftError<NodeId, InstallSnapshotError>>,
    > {
        let target = self.router.get(self.target).await.map_err(RPCError::Network)?;
        target.install_snapshot(rpc).await.map_err(rpc_err_install)
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: openraft::network::RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let target = self.router.get(self.target).await.map_err(RPCError::Network)?;
        target.vote(rpc).await.map_err(rpc_err)
    }
}

fn rpc_err(
    e: openraft::error::RaftError<NodeId>,
) -> RPCError<NodeId, BasicNode, openraft::error::RaftError<NodeId>> {
    RPCError::RemoteError(openraft::error::RemoteError::new(0, e))
}

fn rpc_err_install(
    e: openraft::error::RaftError<NodeId, InstallSnapshotError>,
) -> RPCError<NodeId, BasicNode, openraft::error::RaftError<NodeId, InstallSnapshotError>> {
    RPCError::RemoteError(openraft::error::RemoteError::new(0, e))
}
