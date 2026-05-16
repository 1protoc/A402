//! 4-node Raft committee convergence test over HTTP transport.
//!
//! Same shape as `four_node_convergence.rs` (in-process router) but each
//! peer's RPCs travel through axum/reqwest on distinct loopback ports.
//! Proves slice 2A: the HTTP layer that real deployments will use.
//!
//! Slice 2B adds mTLS by replacing each node's `axum::serve(listener, …)`
//! with a rustls-wrapped listener and pinning peer certs. Wire format is
//! identical, so this test stays.

use std::collections::BTreeMap;
use std::time::Duration;

use a402_raft::{CommitteeConfig, HttpPeerMap, RaftCommittee, WalEvent};
use openraft::BasicNode;
use tokio::net::TcpListener;

async fn bind_free_port() -> (std::net::SocketAddr, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    (addr, listener)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_node_http_convergence() {
    let _ = tracing_subscriber::fmt::try_init();

    // 1. Reserve four free ports.
    let mut addrs = Vec::new();
    let mut listeners = Vec::new();
    for _ in 0..4 {
        let (addr, listener) = bind_free_port().await;
        addrs.push(addr);
        listeners.push(listener);
    }

    let peers_map: BTreeMap<u64, String> = (1u64..=4)
        .zip(&addrs)
        .map(|(id, a)| (id, format!("http://{a}")))
        .collect();
    let peers_basic: BTreeMap<u64, BasicNode> = peers_map
        .iter()
        .map(|(id, url)| (*id, BasicNode::new(url.clone())))
        .collect();

    // 2. Bring up each node + spawn its inbound axum server.
    let mut nodes: Vec<RaftCommittee> = Vec::new();
    let mut server_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    for (id, listener) in (1u64..=4).zip(listeners.into_iter()) {
        let cfg = CommitteeConfig::new(id, peers_basic.clone());
        let (node, router) = RaftCommittee::start_http(cfg, HttpPeerMap::new(peers_map.clone()))
            .await
            .unwrap_or_else(|e| panic!("start_http node {id}: {e}"));
        server_tasks.push(tokio::spawn(async move {
            axum::serve(listener, router).await.expect("axum serve");
        }));
        nodes.push(node);
    }

    // 3. Seed cluster membership from node 1.
    nodes[0]
        .bootstrap(peers_basic.clone())
        .await
        .expect("bootstrap");

    // 4. Wait for leader election.
    let leader_id = nodes[0]
        .wait_for_leader(Duration::from_secs(10))
        .await
        .expect("leader elected");
    eprintln!("[raft-http] leader = node {leader_id}");

    // 5. Propose 10 distinct events through the leader.
    let leader = nodes
        .iter()
        .find(|n| n.node_id == leader_id)
        .expect("leader in nodes");
    for i in 0u32..10 {
        let payload = format!("http-wal-{i:02}").into_bytes();
        leader
            .propose(WalEvent::new(payload))
            .await
            .expect("leader propose");
    }
    eprintln!("[raft-http] 10 events proposed");

    // 6. Wait for followers to apply.
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let mut all_done = true;
        for n in &nodes {
            if n.applied().await.len() < 10 {
                all_done = false;
                break;
            }
        }
        if all_done {
            break;
        }
        if std::time::Instant::now() >= deadline {
            for n in &nodes {
                eprintln!(
                    "[raft-http] node {} applied = {}",
                    n.node_id,
                    n.applied().await.len()
                );
            }
            panic!("HTTP cluster did not converge within deadline");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 7. All 4 nodes must hold identical sequences.
    let expected: Vec<Vec<u8>> = (0u32..10)
        .map(|i| format!("http-wal-{i:02}").into_bytes())
        .collect();
    for n in &nodes {
        let applied: Vec<Vec<u8>> = n
            .applied()
            .await
            .into_iter()
            .map(|e| e.as_bytes().to_vec())
            .collect();
        assert_eq!(applied, expected, "node {} sequence mismatch", n.node_id);
    }
    eprintln!("[raft-http] all 4 nodes converged ✓");

    // 8. Teardown.
    for n in nodes {
        let _ = n.shutdown().await;
    }
    for t in server_tasks {
        t.abort();
    }
}
