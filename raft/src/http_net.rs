//! HTTP transport for Raft RPCs.
//!
//! Each peer runs an axum server that exposes three POST routes:
//!   - `POST /raft/append-entries`
//!   - `POST /raft/vote`
//!   - `POST /raft/install-snapshot`
//!
//! Bodies are `bincode`-serialized openraft requests; responses are the
//! corresponding bincode-serialized openraft responses. Switching to JSON
//! would bloat by ~3x and round-trip bytes awkwardly.
//!
//! This is plain HTTP — slice 2B layers mTLS on top by wrapping the
//! axum listener with rustls and pinning peer certs against the SP-style
//! `Reg_S` registry. Same wire shape, just a different listener.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router as AxumRouter,
};
use openraft::error::{InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError};
use openraft::network::{RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use openraft::{BasicNode, Raft};
use reqwest::Client as HttpClient;

use crate::types::{NodeId, TypeConfig};

/* -------------------------------------------------------------------------- */
/*                              Factory + Client                               */
/* -------------------------------------------------------------------------- */

/// Maps `NodeId → base_url` so the factory can resolve targets without
/// touching `BasicNode` (openraft does pass `BasicNode` to `new_client`,
/// but we accept either: explicit map wins; otherwise fall back to
/// `BasicNode.addr`).
#[derive(Clone, Debug, Default)]
pub struct HttpPeerMap(pub BTreeMap<NodeId, String>);

impl HttpPeerMap {
    pub fn new(map: BTreeMap<NodeId, String>) -> Self {
        Self(map)
    }
}

#[derive(Clone)]
pub struct HttpFactory {
    peers: Arc<HttpPeerMap>,
    http: HttpClient,
}

impl HttpFactory {
    pub fn new(peers: HttpPeerMap) -> Self {
        let http = HttpClient::builder()
            .no_proxy() // bypass macOS Privoxy in dev
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client");
        Self {
            peers: Arc::new(peers),
            http,
        }
    }

    fn resolve(&self, target: NodeId, node: &BasicNode) -> String {
        if let Some(url) = self.peers.0.get(&target) {
            return url.trim_end_matches('/').to_string();
        }
        // Fall back to BasicNode.addr — caller is expected to write the
        // full base URL (e.g. "http://127.0.0.1:9001") into the node addr.
        node.addr.trim_end_matches('/').to_string()
    }
}

impl RaftNetworkFactory<TypeConfig> for HttpFactory {
    type Network = HttpPeer;

    async fn new_client(&mut self, target: NodeId, node: &BasicNode) -> Self::Network {
        HttpPeer {
            target,
            base_url: self.resolve(target, node),
            http: self.http.clone(),
        }
    }
}

pub struct HttpPeer {
    target: NodeId,
    base_url: String,
    http: HttpClient,
}

impl HttpPeer {
    async fn rpc<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &Req,
    ) -> Result<Resp, NetworkError> {
        let url = format!("{}{}", self.base_url, path);
        let bytes = bincode::serialize(body).map_err(|e| {
            NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bincode serialize: {e}"),
            ))
        })?;
        let resp = self
            .http
            .post(&url)
            .body(bytes)
            .send()
            .await
            .map_err(|e| NetworkError::new(&e))?;
        if !resp.status().is_success() {
            return Err(NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "peer {target} {path} → HTTP {status}",
                    target = self.target,
                    status = resp.status()
                ),
            )));
        }
        let body = resp
            .bytes()
            .await
            .map_err(|e| NetworkError::new(&e))?;
        bincode::deserialize(&body).map_err(|e| {
            NetworkError::new(&std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bincode deserialize: {e}"),
            ))
        })
    }
}

impl RaftNetwork<TypeConfig> for HttpPeer {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let envelope: Envelope<AppendEntriesResponse<NodeId>, RaftError<NodeId>> = self
            .rpc("/raft/append-entries", &rpc)
            .await
            .map_err(RPCError::Network)?;
        envelope.into_rpc_result(self.target)
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        _option: openraft::network::RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, BasicNode, RaftError<NodeId, InstallSnapshotError>>,
    > {
        let envelope: Envelope<InstallSnapshotResponse<NodeId>, RaftError<NodeId, InstallSnapshotError>> =
            self.rpc("/raft/install-snapshot", &rpc)
                .await
                .map_err(RPCError::Network)?;
        envelope.into_rpc_result(self.target)
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        _option: openraft::network::RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let envelope: Envelope<VoteResponse<NodeId>, RaftError<NodeId>> = self
            .rpc("/raft/vote", &rpc)
            .await
            .map_err(RPCError::Network)?;
        envelope.into_rpc_result(self.target)
    }
}

/// Wire envelope: bincode-encoded `Result<Resp, RaftError>`. The receiver
/// pattern-matches on this so a remote-side Raft error round-trips back as
/// `RPCError::RemoteError`, not as a transport-level NetworkError.
#[derive(serde::Serialize, serde::Deserialize)]
enum Envelope<R, E> {
    Ok(R),
    Err(E),
}

impl<R, E: std::error::Error> Envelope<R, E> {
    fn into_rpc_result(self, target: NodeId) -> Result<R, RPCError<NodeId, BasicNode, E>> {
        match self {
            Envelope::Ok(r) => Ok(r),
            Envelope::Err(e) => Err(RPCError::RemoteError(RemoteError::new(target, e))),
        }
    }
}

/* -------------------------------------------------------------------------- */
/*                              Receiver (axum)                                */
/* -------------------------------------------------------------------------- */

#[derive(Clone)]
struct ServerState {
    raft: Raft<TypeConfig>,
}

/// Build the axum router that handles inbound Raft RPCs. Mount this on a
/// `tokio::net::TcpListener` of your choosing.
pub fn http_router(raft: Raft<TypeConfig>) -> AxumRouter {
    AxumRouter::new()
        .route("/raft/append-entries", post(handle_append_entries))
        .route("/raft/vote", post(handle_vote))
        .route("/raft/install-snapshot", post(handle_install_snapshot))
        .with_state(ServerState { raft })
}

async fn handle_append_entries(
    State(state): State<ServerState>,
    body: Bytes,
) -> impl IntoResponse {
    let req: AppendEntriesRequest<TypeConfig> = match bincode::deserialize(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("decode: {e}")).into_response(),
    };
    let result = state.raft.append_entries(req).await;
    encode_envelope::<AppendEntriesResponse<NodeId>, RaftError<NodeId>>(result)
}

async fn handle_vote(
    State(state): State<ServerState>,
    body: Bytes,
) -> impl IntoResponse {
    let req: VoteRequest<NodeId> = match bincode::deserialize(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("decode: {e}")).into_response(),
    };
    let result = state.raft.vote(req).await;
    encode_envelope::<VoteResponse<NodeId>, RaftError<NodeId>>(result)
}

async fn handle_install_snapshot(
    State(state): State<ServerState>,
    body: Bytes,
) -> impl IntoResponse {
    let req: InstallSnapshotRequest<TypeConfig> = match bincode::deserialize(&body) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("decode: {e}")).into_response(),
    };
    let result = state.raft.install_snapshot(req).await;
    encode_envelope::<InstallSnapshotResponse<NodeId>, RaftError<NodeId, InstallSnapshotError>>(
        result,
    )
}

fn encode_envelope<R, E>(result: Result<R, E>) -> axum::response::Response
where
    R: serde::Serialize,
    E: serde::Serialize,
{
    let envelope: Envelope<R, E> = match result {
        Ok(r) => Envelope::Ok(r),
        Err(e) => Envelope::Err(e),
    };
    match bincode::serialize(&envelope) {
        Ok(bytes) => (StatusCode::OK, bytes).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("encode: {e}")).into_response(),
    }
}
