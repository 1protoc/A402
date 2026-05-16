//! High-level facade over openraft. The integration test and (later)
//! `enclave/src/main.rs` interact with a `RaftCommittee`, not with the raw
//! `Raft<TypeConfig>` handle.
//!
//! Slice 1 API:
//!   - [`CommitteeConfig::new`]: declare yourself + your peers
//!   - [`RaftCommittee::start`]: bring up the node, register in the [`Router`]
//!   - [`RaftCommittee::bootstrap`]: initialize cluster membership (call once,
//!     on the seed node)
//!   - [`RaftCommittee::propose`]: leader-only — append a [`WalEvent`] to the
//!     replicated log, wait for it to commit
//!   - [`RaftCommittee::applied`]: peek at the state machine for assertions
//!   - [`RaftCommittee::wait_for_leader`]: tests poll until election finishes
//!
//! Errors map openraft's many error types into a single [`RaftError`].

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Router as AxumRouter;
use openraft::storage::Adaptor;
use openraft::{BasicNode, Config, Raft};
use thiserror::Error;

use crate::http_net::{http_router, HttpFactory, HttpPeerMap};
use crate::network::{InProcessFactory, Router};
use crate::store::MemStore;
use crate::types::{NodeId, TypeConfig, WalEvent};

#[derive(Debug, Error)]
pub enum RaftError {
    #[error("raft init: {0}")]
    Init(String),
    #[error("raft propose: {0}")]
    Propose(String),
    #[error("raft membership: {0}")]
    Membership(String),
    #[error("not the leader (current leader = {0:?})")]
    NotLeader(Option<NodeId>),
    #[error("timeout waiting for leader after {0:?}")]
    LeaderTimeout(Duration),
}

#[derive(Clone, Debug)]
pub struct CommitteeConfig {
    pub node_id: NodeId,
    pub peers: BTreeMap<NodeId, BasicNode>,
    /// openraft tuning. `None` → use library defaults tuned for our slice.
    pub raft_config: Option<Config>,
}

impl CommitteeConfig {
    pub fn new(node_id: NodeId, peers: BTreeMap<NodeId, BasicNode>) -> Self {
        Self {
            node_id,
            peers,
            raft_config: None,
        }
    }

    fn resolved_raft_config(&self) -> Result<Arc<Config>, RaftError> {
        let cfg = match &self.raft_config {
            Some(c) => c.clone(),
            None => Config {
                cluster_name: "a402-vault-committee".to_string(),
                // Aggressive timings so the in-process test elects quickly.
                heartbeat_interval: 100,
                election_timeout_min: 300,
                election_timeout_max: 600,
                ..Default::default()
            },
        };
        let validated = cfg
            .validate()
            .map_err(|e| RaftError::Init(format!("config: {e}")))?;
        Ok(Arc::new(validated))
    }
}

pub struct RaftCommittee {
    pub node_id: NodeId,
    pub raft: Raft<TypeConfig>,
    pub store: MemStore,
    pub router: Router,
}

impl RaftCommittee {
    /// Bring up a node using the in-process [`Router`] (slice-1 unit tests).
    pub async fn start(cfg: CommitteeConfig, router: Router) -> Result<Self, RaftError> {
        let raft_config = cfg.resolved_raft_config()?;
        let store = MemStore::new(None);
        let (log_store, state_machine) = Adaptor::new(store.clone());
        let network = InProcessFactory::new(router.clone());

        let raft = Raft::new(
            cfg.node_id,
            raft_config,
            network,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| RaftError::Init(format!("Raft::new: {e}")))?;

        router.register(cfg.node_id, raft.clone()).await;

        Ok(Self {
            node_id: cfg.node_id,
            raft,
            store,
            router,
        })
    }

    /// Bring up a node using HTTP transport. `peers` maps every peer's
    /// `NodeId` to its `http://host:port` base URL. Returns the
    /// [`RaftCommittee`] together with an axum [`Router`] the caller must
    /// mount on a `tokio::net::TcpListener` so inbound RPCs can be served.
    pub async fn start_http(
        cfg: CommitteeConfig,
        peers: HttpPeerMap,
    ) -> Result<(Self, AxumRouter), RaftError> {
        let raft_config = cfg.resolved_raft_config()?;
        let store = MemStore::new(None);
        let (log_store, state_machine) = Adaptor::new(store.clone());
        let network = HttpFactory::new(peers);

        let raft = Raft::new(
            cfg.node_id,
            raft_config,
            network,
            log_store,
            state_machine,
        )
        .await
        .map_err(|e| RaftError::Init(format!("Raft::new: {e}")))?;

        let axum_router = http_router(raft.clone());
        Ok((
            Self {
                node_id: cfg.node_id,
                raft,
                store,
                // The in-process Router is unused on the HTTP path but the
                // field is non-optional; expose an empty placeholder so the
                // shape of the struct doesn't fork.
                router: Router::new(),
            },
            axum_router,
        ))
    }

    /// One node calls this exactly once to seed cluster membership. The other
    /// peers will discover the leader via [`Router`] + heartbeats.
    pub async fn bootstrap(&self, members: BTreeMap<NodeId, BasicNode>) -> Result<(), RaftError> {
        self.raft
            .initialize(members)
            .await
            .map_err(|e| RaftError::Membership(format!("initialize: {e}")))
    }

    /// Block until any node reports a current leader, or `timeout` elapses.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Result<NodeId, RaftError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let metrics = self.raft.metrics().borrow().clone();
            if let Some(leader) = metrics.current_leader {
                return Ok(leader);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(RaftError::LeaderTimeout(timeout));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Append a `WalEvent` to the replicated log. Only the leader accepts
    /// proposals; followers return [`RaftError::NotLeader`].
    pub async fn propose(&self, event: WalEvent) -> Result<(), RaftError> {
        match self.raft.client_write(event).await {
            Ok(_) => Ok(()),
            Err(openraft::error::RaftError::APIError(
                openraft::error::ClientWriteError::ForwardToLeader(fl),
            )) => Err(RaftError::NotLeader(fl.leader_id)),
            Err(e) => Err(RaftError::Propose(format!("{e}"))),
        }
    }

    /// Test helper: read the committed payload sequence on this node.
    pub async fn applied(&self) -> Vec<WalEvent> {
        self.store.applied().await
    }

    pub async fn metrics(&self) -> openraft::RaftMetrics<NodeId, BasicNode> {
        self.raft.metrics().borrow().clone()
    }

    pub async fn shutdown(self) -> Result<(), RaftError> {
        self.raft
            .shutdown()
            .await
            .map_err(|e| RaftError::Init(format!("shutdown: {e}")))
    }
}
