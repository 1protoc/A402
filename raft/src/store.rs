//! In-memory `RaftStorage` v1 impl for slice 1.
//!
//! Implements the older unified [`openraft::RaftStorage`] trait and exposes
//! it via [`openraft::storage::Adaptor::new`] so callers get the v2 pair
//! `(RaftLogStorage, RaftStateMachine)` for `Raft::new`. The v2 traits are
//! sealed in 0.9, so users must go through this adapter.
//!
//! Persistence (disk-backed log + snapshots) lands in slice 2 when we
//! integrate with [`enclave/src/wal.rs`].

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::sync::Arc;

use openraft::storage::Snapshot;
use openraft::{
    BasicNode, Entry, EntryPayload, LogId, LogState, OptionalSend, RaftLogId, RaftLogReader,
    RaftSnapshotBuilder, RaftStorage, SnapshotMeta, StorageError, StoredMembership, Vote,
};
use tokio::sync::{mpsc, RwLock};

use crate::types::{NodeId, TypeConfig, WalEvent};

/// Channel sender used by the state machine to publish committed events.
/// Slice 2 routes this into [`enclave/src/wal.rs`].
pub type AppliedSender = mpsc::UnboundedSender<WalEvent>;

#[derive(Debug, Default)]
struct StoreInner {
    /// log index → entry
    log: BTreeMap<u64, Entry<TypeConfig>>,
    last_purged_log_id: Option<LogId<NodeId>>,
    committed: Option<LogId<NodeId>>,
    vote: Option<Vote<NodeId>>,

    /// State machine: ordered list of all committed payloads + book-keeping.
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    applied: Vec<WalEvent>,

    snapshot_idx: u64,
    current_snapshot: Option<StoredSnapshot>,
}

#[derive(Debug, Clone)]
struct StoredSnapshot {
    meta: SnapshotMeta<NodeId, BasicNode>,
    data: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SmSnapshot {
    last_applied_log: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    applied: Vec<WalEvent>,
}

#[derive(Clone, Debug, Default)]
pub struct MemStore {
    inner: Arc<RwLock<StoreInner>>,
    applied_tx: Option<AppliedSender>,
}

impl MemStore {
    pub fn new(applied_tx: Option<AppliedSender>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner::default())),
            applied_tx,
        }
    }

    /// Test helper: read the committed payload sequence.
    pub async fn applied(&self) -> Vec<WalEvent> {
        self.inner.read().await.applied.clone()
    }

    /// Test helper: last applied log id.
    pub async fn last_applied(&self) -> Option<LogId<NodeId>> {
        self.inner.read().await.last_applied_log
    }
}

impl RaftLogReader<TypeConfig> for MemStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        let inner = self.inner.read().await;
        Ok(inner.log.range(range).map(|(_, e)| e.clone()).collect())
    }
}

impl RaftStorage<TypeConfig> for MemStore {
    type LogReader = Self;
    type SnapshotBuilder = Self;

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        self.inner.write().await.vote = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.read().await.vote)
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        self.inner.write().await.committed = committed;
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        Ok(self.inner.read().await.committed)
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let inner = self.inner.read().await;
        let last = inner
            .log
            .iter()
            .next_back()
            .map(|(_, e)| *e.get_log_id())
            .or(inner.last_purged_log_id);
        Ok(LogState {
            last_purged_log_id: inner.last_purged_log_id,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
    {
        let mut inner = self.inner.write().await;
        for entry in entries {
            inner.log.insert(entry.log_id.index, entry);
        }
        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        let keys: Vec<u64> = inner.log.range(log_id.index..).map(|(k, _)| *k).collect();
        for k in keys {
            inner.log.remove(&k);
        }
        Ok(())
    }

    async fn purge_logs_upto(
        &mut self,
        log_id: LogId<NodeId>,
    ) -> Result<(), StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        inner.last_purged_log_id = Some(log_id);
        let keys: Vec<u64> = inner.log.range(..=log_id.index).map(|(k, _)| *k).collect();
        for k in keys {
            inner.log.remove(&k);
        }
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<
        (
            Option<LogId<NodeId>>,
            StoredMembership<NodeId, BasicNode>,
        ),
        StorageError<NodeId>,
    > {
        let inner = self.inner.read().await;
        Ok((inner.last_applied_log, inner.last_membership.clone()))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry<TypeConfig>],
    ) -> Result<Vec<()>, StorageError<NodeId>> {
        let mut inner = self.inner.write().await;
        let mut out = Vec::new();
        for entry in entries {
            inner.last_applied_log = Some(entry.log_id);
            match &entry.payload {
                EntryPayload::Blank => {}
                EntryPayload::Normal(event) => {
                    inner.applied.push(event.clone());
                    if let Some(tx) = &self.applied_tx {
                        let _ = tx.send(event.clone());
                    }
                }
                EntryPayload::Membership(m) => {
                    inner.last_membership =
                        StoredMembership::new(Some(entry.log_id), m.clone());
                }
            }
            out.push(());
        }
        Ok(out)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        let data = snapshot.into_inner();
        let decoded: SmSnapshot = bincode::deserialize(&data).map_err(|e| {
            StorageError::from_io_error(
                openraft::ErrorSubject::Snapshot(Some(meta.signature())),
                openraft::ErrorVerb::Read,
                std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            )
        })?;
        let mut inner = self.inner.write().await;
        inner.last_applied_log = decoded.last_applied_log;
        inner.last_membership = decoded.last_membership;
        inner.applied = decoded.applied;
        inner.current_snapshot = Some(StoredSnapshot {
            meta: meta.clone(),
            data,
        });
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<TypeConfig>>, StorageError<NodeId>> {
        let inner = self.inner.read().await;
        Ok(inner.current_snapshot.as_ref().map(|s| Snapshot {
            meta: s.meta.clone(),
            snapshot: Box::new(Cursor::new(s.data.clone())),
        }))
    }
}

impl RaftSnapshotBuilder<TypeConfig> for MemStore {
    async fn build_snapshot(&mut self) -> Result<Snapshot<TypeConfig>, StorageError<NodeId>> {
        let (data, meta) = {
            let mut inner = self.inner.write().await;
            inner.snapshot_idx += 1;
            let snap = SmSnapshot {
                last_applied_log: inner.last_applied_log,
                last_membership: inner.last_membership.clone(),
                applied: inner.applied.clone(),
            };
            let bytes = bincode::serialize(&snap).map_err(|e| {
                StorageError::from_io_error(
                    openraft::ErrorSubject::StateMachine,
                    openraft::ErrorVerb::Write,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e),
                )
            })?;
            let snapshot_id = format!(
                "{}-{}-{}",
                inner
                    .last_applied_log
                    .map(|id| id.leader_id.term)
                    .unwrap_or(0),
                inner.last_applied_log.map(|id| id.index).unwrap_or(0),
                inner.snapshot_idx
            );
            let meta = SnapshotMeta {
                last_log_id: inner.last_applied_log,
                last_membership: inner.last_membership.clone(),
                snapshot_id,
            };
            inner.current_snapshot = Some(StoredSnapshot {
                meta: meta.clone(),
                data: bytes.clone(),
            });
            (bytes, meta)
        };
        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}
