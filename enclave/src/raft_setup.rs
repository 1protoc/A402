//! Optional Raft committee bootstrap for the Vault.
//!
//! When the operator wants a 4-node Vault committee, they set:
//!
//!   A402_RAFT_NODE_ID    = 1                                # this node's id
//!   A402_RAFT_LISTEN     = 0.0.0.0:9101                     # this node's raft RPC bind
//!   A402_RAFT_PEERS      = 1@http://10.0.0.1:9101,\
//!                          2@http://10.0.0.2:9101,\
//!                          3@http://10.0.0.3:9101,\
//!                          4@http://10.0.0.4:9101
//!   A402_RAFT_BOOTSTRAP  = 1     # set on exactly ONE seed node, the first time
//!
//! When `A402_RAFT_PEERS` is empty (or unset), the Vault runs in legacy
//! single-node mode — every `/v1/verify` and `/v1/settle` writes to the
//! local WAL synchronously, no committee involvement. This matches the
//! "保留之前的单节点版本" requirement.
//!
//! Slice 2B (this module) brings the committee up and serves Raft RPCs on
//! the side. Slice 2C will route WAL events through `RaftCommittee::propose`
//! before applying to state — that's the change that requires the per-event
//! agreement before the HTTP handler returns 200.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use a402_raft::{BasicNode, CommitteeConfig, HttpPeerMap, RaftCommittee};
use tracing::{info, warn};

#[derive(Debug)]
pub struct RaftEnv {
    pub node_id: u64,
    pub listen: SocketAddr,
    /// `node_id → base URL` for every peer, **including** self.
    pub peers: BTreeMap<u64, String>,
    pub bootstrap: bool,
}

impl RaftEnv {
    /// Parses the four `A402_RAFT_*` env vars. Returns `Ok(None)` when
    /// `A402_RAFT_PEERS` is empty (single-node mode). Returns `Err` on
    /// malformed input so the operator notices immediately.
    pub fn from_env() -> Result<Option<Self>, String> {
        let peers_str = std::env::var("A402_RAFT_PEERS").unwrap_or_default();
        if peers_str.trim().is_empty() {
            return Ok(None);
        }

        let peers = parse_peers(&peers_str)?;
        let node_id = std::env::var("A402_RAFT_NODE_ID")
            .map_err(|_| "A402_RAFT_PEERS is set, so A402_RAFT_NODE_ID must be set too".to_string())?
            .parse::<u64>()
            .map_err(|e| format!("A402_RAFT_NODE_ID must be a u64: {e}"))?;
        if !peers.contains_key(&node_id) {
            return Err(format!(
                "A402_RAFT_NODE_ID = {node_id} but that id is not in A402_RAFT_PEERS"
            ));
        }
        let listen = std::env::var("A402_RAFT_LISTEN")
            .map_err(|_| "A402_RAFT_PEERS is set, so A402_RAFT_LISTEN must be set too".to_string())?
            .parse::<SocketAddr>()
            .map_err(|e| format!("A402_RAFT_LISTEN must be a SocketAddr: {e}"))?;
        let bootstrap = matches!(
            std::env::var("A402_RAFT_BOOTSTRAP").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
        );

        Ok(Some(RaftEnv {
            node_id,
            listen,
            peers,
            bootstrap,
        }))
    }
}

fn parse_peers(input: &str) -> Result<BTreeMap<u64, String>, String> {
    let mut out = BTreeMap::new();
    for raw in input.split(',') {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        let (id_str, url) = entry
            .split_once('@')
            .ok_or_else(|| format!("peer entry '{entry}' must be 'id@url'"))?;
        let id = id_str
            .parse::<u64>()
            .map_err(|e| format!("peer id '{id_str}' is not a u64: {e}"))?;
        let url = url.trim().to_string();
        if url.is_empty() {
            return Err(format!("peer {id} has empty url"));
        }
        out.insert(id, url);
    }
    if out.is_empty() {
        return Err("A402_RAFT_PEERS parsed empty after trimming".to_string());
    }
    Ok(out)
}

/// Brings up a raft committee node and spawns its inbound RPC server.
/// Returns the `RaftCommittee` so the caller can later route WAL events
/// through it (slice 2C).
pub async fn start_committee(env: RaftEnv) -> Result<RaftCommittee, String> {
    let peers_basic: BTreeMap<u64, BasicNode> = env
        .peers
        .iter()
        .map(|(id, url)| (*id, BasicNode::new(url.clone())))
        .collect();
    let peers_map = HttpPeerMap::new(env.peers.clone());

    let cfg = CommitteeConfig::new(env.node_id, peers_basic.clone());
    let (committee, axum_router) = RaftCommittee::start_http(cfg, peers_map)
        .await
        .map_err(|e| format!("RaftCommittee::start_http: {e}"))?;

    // Spawn the raft RPC server (separate axum task, separate port from
    // the Vault's /v1/* surface).
    let listener = tokio::net::TcpListener::bind(env.listen)
        .await
        .map_err(|e| format!("bind raft listener at {}: {e}", env.listen))?;
    info!(node_id = env.node_id, raft_listen = %env.listen, peers = ?env.peers, "Raft committee starting");
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, axum_router).await {
            warn!("raft RPC server stopped: {e}");
        }
    });

    if env.bootstrap {
        info!(
            "A402_RAFT_BOOTSTRAP=1 → initializing cluster membership ({} peers)",
            peers_basic.len()
        );
        committee
            .bootstrap(peers_basic)
            .await
            .map_err(|e| format!("raft bootstrap: {e}"))?;
    }

    // Best-effort: log the leader once it's known, for operator visibility.
    let metrics = committee.raft.metrics().clone();
    let node_id = env.node_id;
    tokio::spawn(async move {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(leader) = metrics.borrow().current_leader {
                info!(node_id, %leader, "Raft leader observed");
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                warn!(node_id, "Raft leader still unknown after 15s");
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });

    Ok(committee)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_peers_csv() {
        let m = parse_peers("1@http://a:1, 2@http://b:2 ,3@http://c:3").unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m.get(&1).unwrap(), "http://a:1");
        assert_eq!(m.get(&2).unwrap(), "http://b:2");
        assert_eq!(m.get(&3).unwrap(), "http://c:3");
    }

    #[test]
    fn rejects_missing_at() {
        let err = parse_peers("1=http://a:1").unwrap_err();
        assert!(err.contains("'id@url'"), "got: {err}");
    }

    #[test]
    fn rejects_bad_id() {
        let err = parse_peers("not-a-number@http://a:1").unwrap_err();
        assert!(err.contains("is not a u64"), "got: {err}");
    }
}
