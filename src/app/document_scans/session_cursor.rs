use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::mem::size_of;
use std::sync::Arc;

use crate::midge::adapter::{AccountedDocument, DocumentRef, Midge, MidgeRowCursor, RowDecode};
use crate::runtime::accounted::{json, Accounted, AccountedVec};
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
            snapshot.estimated_retained_bytes()?
        };
        let staged_changes =
            Accounted::try_new(controls, retained_bytes, || snapshot.shared_changes())?;
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
                    push_accounted_document(
                        &mut output,
                        self.persisted_pending
                            .take()
                            .expect("pending persisted row"),
                    )?;
                }
                (None, Some(_)) => {
                    if let Some(document) = self.take_staged_document(controls)? {
                        push_accounted_document(&mut output, document)?;
                    }
                }
                (Some(persisted_id), Some(staged_id)) => match persisted_id.cmp(staged_id) {
                    Ordering::Less => {
                        push_accounted_document(
                            &mut output,
                            self.persisted_pending
                                .take()
                                .expect("pending persisted row"),
                        )?;
                    }
                    Ordering::Greater => {
                        if let Some(document) = self.take_staged_document(controls)? {
                            push_accounted_document(&mut output, document)?;
                        }
                    }
                    Ordering::Equal => {
                        drop(self.persisted_pending.take());
                        if let Some(document) = self.take_staged_document(controls)? {
                            push_accounted_document(&mut output, document)?;
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
        let retained_bytes = staged_document_retained_bytes(id, payload, &self.projection)?;
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

fn push_accounted_document(
    documents: &mut Vec<AccountedDocument>,
    document: AccountedDocument,
) -> Result<(), CassieError> {
    // Each document already reserves its inline slot. Exact growth avoids retaining
    // additional slots outside those guards, including on a final one-row page.
    documents.try_reserve_exact(1).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to retain controlled session page: {error}"))
    })?;
    documents.push(document);
    Ok(())
}

fn staged_document_retained_bytes(
    id: &str,
    payload: &serde_json::Value,
    projection: &StagedProjection,
) -> Result<usize, CassieError> {
    let projection_names = match projection {
        StagedProjection::Full => 0,
        StagedProjection::Fields(fields) => fields.iter().try_fold(0usize, |bytes, field| {
            bytes
                .checked_add(field.len())
                .ok_or_else(accounting_overflow)
        })?,
    };
    [
        size_of::<AccountedDocument>(),
        id.len(),
        json::retained_bytes(payload)?,
        projection_names,
    ]
    .into_iter()
    .try_fold(0usize, |bytes, retained| {
        bytes.checked_add(retained).ok_or_else(accounting_overflow)
    })
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("staged cursor retained memory accounting overflow".to_owned())
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;
    use crate::midge::adapter::query_scan_control_test_guard;
    use crate::types::Schema;

    use super::{
        staged_document_retained_bytes, AccountedDocument, CassieError, DocumentRef, Midge,
        QueryExecutionControls, RowDecode, SessionRowCursor, StagedProjection,
    };

    #[test]
    fn should_return_session_cursor_pages_without_unaccounted_spare_document_slots() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let path = std::env::temp_dir().join(format!(
            "cassie-session-cursor-page-capacity-{}",
            uuid::Uuid::new_v4()
        ));
        let midge = Midge::new_strict_with_data_dir(&path).expect("local Midge");
        midge.ensure_families_ready().expect("storage families");
        let collection = "session_cursor_page_capacity";
        midge
            .create_collection(collection, Schema { fields: Vec::new() })
            .expect("RowStore collection");
        for id in ["one", "two"] {
            midge
                .put_document(collection, Some(id.to_owned()), serde_json::json!({}))
                .expect("identity-only row");
        }
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let persisted = midge
            .open_row_cursor(collection, RowDecode::Full)
            .expect("open persisted cursor")
            .expect("RowStore cursor");
        let mut cursor =
            SessionRowCursor::new(None, collection, persisted, RowDecode::Full, &controls)
                .expect("session cursor");

        // Act
        let first = cursor
            .next_accounted_documents(&midge, 1, &controls)
            .expect("first one-row page");
        let second = cursor
            .next_accounted_documents(&midge, 1, &controls)
            .expect("second one-row page");
        drop(cursor);

        // Assert
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].id(), "one");
        assert_eq!(second[0].id(), "two");
        let document_bytes = first[0].accounted_bytes() + second[0].accounted_bytes();
        assert_eq!(controls.current_query_memory_bytes(), document_bytes);
        assert_eq!(
            first.capacity(),
            first.len(),
            "the returned Vec has only its retained document slots reserved"
        );
        assert_eq!(second.capacity(), second.len());
        drop(first);
        drop(second);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop(midge);
        std::fs::remove_dir_all(path).expect("remove storage fixture");
    }

    #[test]
    fn should_reserve_sparse_staged_json_before_cloning_a_cursor_document() {
        // Arrange
        let mut nested = serde_json::Value::Null;
        for _ in 0..32 {
            nested = serde_json::json!({"a": nested});
        }
        let payload = serde_json::json!({"payload": nested});
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 16 * 1_024,
            ..CassieRuntimeLimits::default()
        };
        let projections = [
            StagedProjection::Full,
            StagedProjection::Fields(vec!["payload".to_owned()]),
        ];

        for projection in &projections {
            let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
            let clone_calls = Cell::new(0);
            let retained_bytes = staged_document_retained_bytes("one", &payload, projection)
                .expect("staged document retained estimate");

            // Act
            let result = AccountedDocument::try_build(&controls, retained_bytes, || {
                clone_calls.set(clone_calls.get() + 1);
                let payload = match projection {
                    StagedProjection::Full => payload.clone(),
                    StagedProjection::Fields(fields) => {
                        super::project_payload_fields(&payload, fields)
                    }
                };
                Ok(DocumentRef {
                    id: "one".to_owned(),
                    payload,
                })
            });

            // Assert
            assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
            assert_eq!(clone_calls.get(), 0, "reserve before the staged clone");
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
