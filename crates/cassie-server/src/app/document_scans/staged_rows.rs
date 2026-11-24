//! The open transaction's staged rows of one collection, in row-id order.
//!
//! A vector scan merges them serially into the persisted lanes (#1358, D5): an
//! upsert replaces or inserts a row, a delete drops it.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::runtime::accounted::{Accounted, AccountedVec};
use crate::runtime::QueryExecutionControls;

use super::super::{CassieError, CassieSession, TransactionRowChange};

type StagedChanges = BTreeMap<String, TransactionRowChange>;

/// A stable snapshot of a session's staged changes for one collection.
pub(crate) struct StagedRowSet {
    changes: Accounted<Arc<StagedChanges>>,
    ids: AccountedVec<String>,
    index: usize,
}

impl StagedRowSet {
    /// Snapshots the session's staged rows of `collection`; a session without
    /// staged changes yields an empty set.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error when the snapshot exceeds the query budget.
    pub(crate) fn open(
        session: Option<&CassieSession>,
        collection: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let snapshot = session.map_or_else(super::super::StagedWriteSnapshot::default, |session| {
            session.staged_write_snapshot(collection)
        });
        let retained_bytes = if snapshot.is_empty() {
            0
        } else {
            snapshot.estimated_retained_bytes()?
        };
        let changes = Accounted::try_new(controls, retained_bytes, || snapshot.shared_changes())?;
        let mut ids = AccountedVec::try_new(controls)?;
        for id in changes.get().keys() {
            ids.try_push_clone(id, id.len())?;
        }
        Ok(Self {
            changes,
            ids,
            index: 0,
        })
    }

    /// Every staged row id, in id order.
    pub(crate) fn ids(&self) -> &[String] {
        self.ids.as_slice()
    }

    /// Whether any staged row has not been taken yet.
    pub(crate) fn has_pending(&self) -> bool {
        self.index < self.ids.len()
    }

    /// The id of the next staged row.
    pub(crate) fn peek_id(&self) -> Option<&str> {
        self.ids.as_slice().get(self.index).map(String::as_str)
    }

    /// Takes the next staged row, handing its id and payload (`None` for a
    /// delete) to `read`.
    pub(crate) fn take_next<R>(
        &mut self,
        read: impl FnOnce(&str, Option<&serde_json::Value>) -> R,
    ) -> Option<R> {
        let id = self.ids.as_slice().get(self.index)?.as_str();
        self.index = self.index.saturating_add(1);
        let payload = match self.changes.get().get(id)? {
            TransactionRowChange::Upsert(payload) => Some(payload),
            TransactionRowChange::Delete => None,
        };
        Some(read(id, payload))
    }
}
