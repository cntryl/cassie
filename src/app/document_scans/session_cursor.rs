use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::Arc;

use crate::midge::adapter::{AccountedDocument, DocumentRef, Midge, MidgeRowCursor, RowDecode};
use crate::runtime::accounted::{Accounted, AccountedVec};
use crate::runtime::QueryExecutionControls;

use super::super::vector_helpers::project_payload_fields;
use super::super::{CassieError, CassieSession, TransactionRowChange};

type StagedChanges = BTreeMap<String, TransactionRowChange>;

#[derive(Debug)]
enum StagedProjection {
    Full,
    Fields(Vec<String>),
}

/// Merge-walks one stable transaction snapshot with the persisted row-ID ordering.
pub(crate) struct SessionRowCursor {
    persisted: MidgeRowCursor,
    persisted_pending: Option<AccountedDocument>,
    staged_changes: Accounted<Arc<StagedChanges>>,
    staged_ids: AccountedVec<String>,
    staged_index: usize,
    projection: StagedProjection,
}

impl std::fmt::Debug for SessionRowCursor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionRowCursor")
            .field("persisted", &self.persisted)
            .field("has_persisted_pending", &self.persisted_pending.is_some())
            .field("staged_rows", &self.staged_ids.len())
            .field("staged_empty", &self.staged_ids.is_empty())
            .field("snapshot_bytes", &self.staged_changes.accounted_bytes())
            .field("staged_key_bytes", &self.staged_ids.accounted_bytes())
            .field("staged_index", &self.staged_index)
            .finish_non_exhaustive()
    }
}

impl SessionRowCursor {
    pub(super) fn new(
        session: Option<&CassieSession>,
        collection: &str,
        persisted: MidgeRowCursor,
        decode: RowDecode,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let snapshot = session.map_or_else(super::super::StagedWriteSnapshot::default, |session| {
            session.staged_write_snapshot(collection)
        });
        Self::from_snapshot(&snapshot, persisted, decode, controls)
    }

    fn from_snapshot(
        snapshot: &super::super::StagedWriteSnapshot,
        persisted: MidgeRowCursor,
        decode: RowDecode,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        let retained_bytes = if snapshot.is_empty() {
            0
        } else {
            snapshot.estimated_retained_bytes()
        };
        let shared_changes = snapshot.shared_changes();
        let staged_changes = Accounted::try_new(controls, retained_bytes, || shared_changes)?;
        let mut staged_ids = AccountedVec::try_new(controls)?;
        for id in staged_changes.get().keys() {
            staged_ids.try_push_clone(id, id.len())?;
        }
        let projection = match decode {
            RowDecode::Full => StagedProjection::Full,
            RowDecode::Projected(fields) | RowDecode::ProjectedHistorical(fields) => {
                StagedProjection::Fields(fields)
            }
        };
        Ok(Self {
            persisted,
            persisted_pending: None,
            staged_changes,
            staged_ids,
            staged_index: 0,
            projection,
        })
    }

    pub(crate) fn next_accounted_documents(
        &mut self,
        midge: &Midge,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<AccountedDocument>, CassieError> {
        let mut output = Vec::new();
        while output.len() < limit {
            check_controls(controls)?;
            if self.persisted_pending.is_none() {
                self.persisted_pending = self.persisted.next_accounted_document(midge, controls)?;
            }
            let persisted_id = self.persisted_pending.as_ref().map(AccountedDocument::id);
            let staged_id = self.staged_ids.as_slice().get(self.staged_index);

            match (persisted_id, staged_id) {
                (None, None) => {
                    self.staged_ids.clear();
                    break;
                }
                (Some(_), None) => {
                    output.push(
                        self.persisted_pending
                            .take()
                            .expect("pending persisted row"),
                    );
                }
                (None, Some(_)) => {
                    if let Some(document) = self.take_staged_document(controls)? {
                        output.push(document);
                    }
                }
                (Some(persisted_id), Some(staged_id)) => match persisted_id.cmp(staged_id) {
                    Ordering::Less => {
                        output.push(
                            self.persisted_pending
                                .take()
                                .expect("pending persisted row"),
                        );
                    }
                    Ordering::Greater => {
                        if let Some(document) = self.take_staged_document(controls)? {
                            output.push(document);
                        }
                    }
                    Ordering::Equal => {
                        drop(self.persisted_pending.take());
                        if let Some(document) = self.take_staged_document(controls)? {
                            output.push(document);
                        }
                    }
                },
            }
        }
        Ok(output)
    }

    fn take_staged_document(
        &mut self,
        controls: &QueryExecutionControls,
    ) -> Result<Option<AccountedDocument>, CassieError> {
        let id = self
            .staged_ids
            .as_slice()
            .get(self.staged_index)
            .expect("staged cursor index")
            .as_str();
        self.staged_index = self.staged_index.saturating_add(1);
        let change = self
            .staged_changes
            .get()
            .get(id)
            .expect("staged cursor key");
        let TransactionRowChange::Upsert(payload) = change else {
            return Ok(None);
        };
        let retained_bytes = staged_document_retained_bytes(id, payload, &self.projection);
        AccountedDocument::try_build(controls, retained_bytes, || {
            let payload = match &self.projection {
                StagedProjection::Full => payload.clone(),
                StagedProjection::Fields(fields) => project_payload_fields(payload, fields),
            };
            Ok(DocumentRef {
                id: id.to_string(),
                payload,
            })
        })
        .map(Some)
    }
}

fn staged_document_retained_bytes(
    id: &str,
    payload: &serde_json::Value,
    projection: &StagedProjection,
) -> usize {
    let projection_names = match projection {
        StagedProjection::Full => 0,
        StagedProjection::Fields(fields) => fields.iter().map(String::len).sum(),
    };
    size_of::<AccountedDocument>()
        .saturating_add(id.len())
        .saturating_add(json_retained_bytes(payload))
        .saturating_add(projection_names)
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let inline = size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            inline
        }
        serde_json::Value::String(value) => inline.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(inline, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => values.iter().fold(inline, |bytes, (key, value)| {
            bytes
                .saturating_add(size_of::<String>())
                .saturating_add(key.len())
                .saturating_add(json_retained_bytes(value))
        }),
    }
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}
