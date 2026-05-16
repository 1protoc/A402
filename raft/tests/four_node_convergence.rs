//! 4-node Raft committee end-to-end test.
//!
//! What this test proves for slice 1:
//!   1. We can bring up four [`RaftCommittee`] instances sharing a [`Router`]
//!      and bootstrap them as a quorum (3-of-4 = quorum, matching the
//!      project's intended deployment shape).
//!   2. The cluster elects a leader within the configured timeouts.
//!   3. Proposing N [`WalEvent`]s through the leader replicates them to all
//!      four nodes, in identical order.
//!   4. Proposing on a follower returns [`RaftError::NotLeader`] with the
//!      current leader's id.
//!
//! Slice 2 will swap [`crate::network::InProcessFactory`] for an
//! axum/reqwest mTLS transport but keep the rest of this test verbatim.

use std::collections::BTreeMap;
use std::time::Duration;

use a402_raft::{CommitteeConfig, RaftCommittee, RaftError, Router, WalEvent};
use openraft::BasicNode;

fn basic_peers() -> BTreeMap<u64, BasicNode> {
    (1u64..=4)
        .map(|id| (id, BasicNode::new(format!("inproc://node-{id}"))))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_node_convergence() {
    // Quiet but observable.
    let _ = tracing_subscriber::fmt::try_init();

    let peers = basic_peers();
    let router = Router::new();

    // 1. Bring up all four nodes.
    let mut nodes: Vec<RaftCommittee> = Vec::new();
    for id in 1u64..=4 {
        let cfg = CommitteeConfig::new(id, peers.clone());
        let node = RaftCommittee::start(cfg, router.clone()).await.expect("start");
        nodes.push(node);
    }

    // 2. Bootstrap cluster membership from node 1 (the seed).
    nodes[0].bootstrap(peers.clone()).await.expect("bootstrap");

    // 3. Wait for a leader to emerge.
    let leader_id = nodes[0]
        .wait_for_leader(Duration::from_secs(5))
        .await
        .expect("leader elected");
    eprintln!("[raft] leader = node {leader_id}");

    // 4. Propose 10 events through the leader. Each event carries a unique
    //    payload so we can assert ordering.
    let leader = nodes
        .iter()
        .find(|n| n.node_id == leader_id)
        .expect("leader in nodes");
    for i in 0u32..10 {
        let payload = format!("wal-event-{i:02}").into_bytes();
        leader
            .propose(WalEvent::new(payload))
            .await
            .expect("leader propose");
    }
    eprintln!("[raft] 10 events proposed by leader {leader_id}");

    // 5. Give followers a moment to catch up, then assert all 4 nodes
    //    converged on the identical sequence.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let expected: Vec<Vec<u8>> = (0u32..10)
        .map(|i| format!("wal-event-{i:02}").into_bytes())
        .collect();
    for node in &nodes {
        let applied: Vec<Vec<u8>> = node
            .applied()
            .await
            .into_iter()
            .map(|e| e.as_bytes().to_vec())
            .collect();
        assert_eq!(
            applied.len(),
            expected.len(),
            "node {} applied {} events, expected {}",
            node.node_id,
            applied.len(),
            expected.len()
        );
        assert_eq!(applied, expected, "node {} sequence mismatch", node.node_id);
    }
    eprintln!("[raft] all 4 nodes converged on 10 identical events");

    // 6. Proposing on a non-leader must return NotLeader pointing at the
    //    real leader.
    let follower = nodes
        .iter()
        .find(|n| n.node_id != leader_id)
        .expect("at least one follower");
    let err = follower
        .propose(WalEvent::new(b"reject-me".to_vec()))
        .await
        .unwrap_err();
    match err {
        RaftError::NotLeader(Some(id)) => {
            assert_eq!(id, leader_id, "NotLeader must point to current leader");
            eprintln!("[raft] follower {} correctly rejected, leader hint = {id}", follower.node_id);
        }
        RaftError::NotLeader(None) => {
            // Acceptable race: leader may have stepped down between metrics
            // snapshot and our propose. Still a "not leader" result.
            eprintln!("[raft] follower {} rejected with no leader hint (acceptable)", follower.node_id);
        }
        other => panic!("expected NotLeader, got {other:?}"),
    }

    // 7. Tear down.
    for node in nodes {
        let _ = node.shutdown().await;
    }
}
