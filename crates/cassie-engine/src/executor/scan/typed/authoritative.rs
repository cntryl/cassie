//! Authoritative column vector source of the typed scan (#1358).
//!
//! A column-authoritative table reads lane batches straight from its field
//! keys: no row document is built and no JSON is produced for integers,
//! floats, Booleans or text. Rows the open transaction staged are merged
//! serially in id order (an upsert replaces or inserts a row, a delete drops
//! it), so a transaction sees its own writes exactly as the document path does.

use super::{
    controlled_storage_error, conversion, profile_batch, projected_field_value,
    try_json_to_typed_value, Cassie, CassieSession, DataType, ProfileLanes, QueryError,
    QueryExecutionControls, Value,
};
use crate::app::StagedRowSet;
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::executor::typed_batch::column_vector_eligibility::type_eligibility;
use crate::executor::typed_batch::{Column, TypedBatch};
use crate::midge::adapter::{
    identity_lanes, ColumnVectorBatch, ColumnVectorOutput, ColumnVectorReader,
    COLUMN_VECTOR_BATCH_ROWS,
};
use crate::types::lanes::RawLanes;
use crate::types::row_identity::is_row_identity_column;
use std::mem::size_of;

mod partitions;
pub(crate) use partitions::{PartitionBatches, PartitionedScan, Partitioning};

pub(super) struct AuthoritativeSource {
    reader: ColumnVectorReader,
    staged: StagedRowSet,
    current: Option<(ColumnVectorBatch, usize)>,
    persisted_done: bool,
}

/// Opens the vector source when the collection is column-authoritative and
/// every requested field is a plain stored field of a supported type.
pub(super) fn open(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
    schema: &[(String, DataType)],
    traversal: crate::midge::adapter::RowTraversal,
    controls: &QueryExecutionControls,
) -> Result<Option<AuthoritativeSource>, QueryError> {
    if !cassie.midge.column_vector_enabled()
        || !cassie
            .catalog
            .collection_storage_mode(collection)
            .is_some_and(
                crate::catalog::collections::CollectionStorageMode::uses_column_store_storage,
            )
    {
        return Ok(None);
    }
    let stored = schema
        .iter()
        .filter(|(name, _)| !is_row_identity_column(name))
        .map(|(_, data_type)| data_type);
    if type_eligibility(stored) != crate::executor::typed_batch::ColumnVectorEligibility::Eligible {
        return Ok(None);
    }
    let requested = fields
        .iter()
        .map(|field| (!is_row_identity_column(field)).then_some(field.as_str()))
        .collect::<Vec<_>>();
    let Some(reader) = cassie
        .midge
        .open_column_vector_reader(collection, &requested, traversal)
        .map_err(|error| controlled_storage_error(cassie, error))?
    else {
        return Ok(None);
    };
    let staged = StagedRowSet::open(session, collection, controls)
        .map_err(|error| controlled_storage_error(cassie, error))?;
    Ok(Some(AuthoritativeSource {
        reader,
        staged,
        current: None,
        persisted_done: false,
    }))
}

impl AuthoritativeSource {
    pub(super) fn set_pruning(
        &mut self,
        cassie: &Cassie,
        filter: &crate::midge::adapter::ColumnBatchScanFilter,
        controls: &QueryExecutionControls,
    ) {
        let staged = self
            .staged
            .ids()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        self.reader
            .set_pruning(&cassie.midge, filter, &staged, &controls.storage_request());
    }

    pub(super) fn next_batch(
        &mut self,
        cassie: &Cassie,
        schema: &[(String, DataType)],
        max_rows: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Option<TypedBatch>, QueryError> {
        conversion::check_controls(controls)?;
        let limit = max_rows.clamp(1, COLUMN_VECTOR_BATCH_ROWS);
        if self.current.is_none() {
            self.load_batch(cassie, limit, controls)?;
        }
        let whole = self.current.as_ref().is_some_and(|(batch, row)| {
            *row == 0
                && batch.ids.last().is_some_and(|last| {
                    self.staged
                        .peek_id()
                        .is_none_or(|staged| staged > last.as_str())
                })
        });
        if whole {
            let (batch, _) = self.current.take().expect("current batch");
            return self.lane_batch(cassie, schema, batch, controls).map(Some);
        }
        if self.current.is_none() && !self.staged.has_pending() {
            return Ok(None);
        }
        self.merged_batch(cassie, schema, limit, controls)
    }

    fn load_batch(
        &mut self,
        cassie: &Cassie,
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<(), QueryError> {
        if self.persisted_done {
            return Ok(());
        }
        match self
            .reader
            .next_batch(&cassie.midge, limit, &controls.storage_request())
            .map_err(|error| controlled_storage_error(cassie, error.into()))?
        {
            Some(batch) => self.current = Some((batch, 0)),
            None => self.persisted_done = true,
        }
        Ok(())
    }

    /// A persisted batch with no staged row inside it: its lanes become the
    /// batch's columns without any per-row conversion.
    fn lane_batch(
        &self,
        cassie: &Cassie,
        schema: &[(String, DataType)],
        batch: ColumnVectorBatch,
        controls: &QueryExecutionControls,
    ) -> Result<TypedBatch, QueryError> {
        lane_batch(cassie, self.reader.outputs(), schema, batch, controls)
    }

    /// Merges persisted rows with staged rows in id order into one batch of at
    /// most `limit` rows, converting each row to values.
    fn merged_batch(
        &mut self,
        cassie: &Cassie,
        schema: &[(String, DataType)],
        limit: usize,
        controls: &QueryExecutionControls,
    ) -> Result<Option<TypedBatch>, QueryError> {
        let mut columns: Vec<Vec<Value>> = vec![Vec::new(); schema.len()];
        let mut memory = controls.reserve_query_memory(mul(
            schema.len(),
            add(size_of::<Vec<Value>>(), mul(limit, size_of::<Value>())?)?,
        )?)?;
        let mut rows = 0usize;
        while rows < limit {
            conversion::check_controls(controls)?;
            if self.current.is_none() {
                self.load_batch(cassie, limit - rows, controls)?;
            }
            let persisted = self
                .current
                .as_ref()
                .map(|(batch, row)| batch.ids[*row].as_str());
            let order = match (persisted, self.staged.peek_id()) {
                (None, None) => break,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(persisted), Some(staged)) => persisted.cmp(staged),
            };
            if order == std::cmp::Ordering::Less {
                rows += self.push_persisted(&mut columns, &mut memory)?;
                continue;
            }
            if order == std::cmp::Ordering::Equal {
                self.skip_persisted();
            }
            let counters = cassie.midge.column_vector_counters();
            counters.record_staged_row();
            let pushed = self
                .staged
                .take_next(|id, payload| {
                    payload
                        .map(|payload| push_staged(&mut columns, &mut memory, schema, id, payload))
                        .transpose()
                })
                .transpose()?
                .flatten();
            rows += usize::from(pushed.is_some());
        }
        if rows == 0 {
            return Ok(None);
        }
        let batch = TypedBatch::from_columns(controls, schema, &columns, rows, None)?;
        cassie.runtime.record_storage_access("data", false, true);
        profile_batch(
            "authoritative-merged",
            &batch,
            ProfileLanes {
                values: rows.saturating_mul(schema.len()),
                ..ProfileLanes::default()
            },
        );
        Ok(Some(batch))
    }

    fn skip_persisted(&mut self) {
        if let Some((batch, row)) = self.current.as_mut() {
            *row += 1;
            if *row >= batch.ids.len() {
                self.current = None;
            }
        }
    }

    fn push_persisted(
        &mut self,
        columns: &mut [Vec<Value>],
        memory: &mut crate::runtime::QueryMemoryReservation,
    ) -> Result<usize, QueryError> {
        let Some((batch, row)) = self.current.as_ref() else {
            return Ok(0);
        };
        for (column, output) in columns.iter_mut().zip(self.reader.outputs()) {
            let value = match output {
                ColumnVectorOutput::Identity => Value::String(batch.ids[*row].clone()),
                ColumnVectorOutput::Lane(index) => batch.lanes[*index].value_at(*row),
            };
            memory.try_grow(value_clone_bytes(&value)?)?;
            column.push(value);
        }
        self.skip_persisted();
        Ok(1)
    }
}

/// Turns one persisted lane batch into a typed batch. It reads only shared
/// state, so partition workers call it on their own threads.
pub(super) fn lane_batch(
    cassie: &Cassie,
    outputs: &[ColumnVectorOutput],
    schema: &[(String, DataType)],
    batch: ColumnVectorBatch,
    controls: &QueryExecutionControls,
) -> Result<TypedBatch, QueryError> {
    let ColumnVectorBatch {
        ids, lanes, memory, ..
    } = batch;
    let rows = ids.len();
    let mut uses = vec![0usize; lanes.len()];
    for output in outputs {
        if let ColumnVectorOutput::Lane(index) = output {
            uses[*index] += 1;
        }
    }
    let mut slots = lanes
        .into_iter()
        .map(Some)
        .collect::<Vec<Option<RawLanes>>>();
    let mut columns = Vec::with_capacity(schema.len());
    for ((_, data_type), output) in schema.iter().zip(outputs) {
        let lanes = match output {
            ColumnVectorOutput::Identity => identity_lanes(&ids),
            ColumnVectorOutput::Lane(index) => {
                uses[*index] -= 1;
                let slot = &mut slots[*index];
                if uses[*index] == 0 {
                    slot.take()
                } else {
                    slot.clone()
                }
                .ok_or_else(|| QueryError::General("column vector lane reused".to_owned()))?
            }
        };
        columns.push(Column::from_lanes(controls, data_type, lanes)?);
    }
    drop(slots);
    drop(memory);
    let batch = TypedBatch::from_views(controls, schema, &columns, rows, None)?;
    cassie.runtime.record_storage_access("data", false, true);
    profile_batch(
        "authoritative",
        &batch,
        ProfileLanes {
            values: rows.saturating_mul(schema.len()),
            ..ProfileLanes::default()
        },
    );
    Ok(batch)
}

fn push_staged(
    columns: &mut [Vec<Value>],
    memory: &mut crate::runtime::QueryMemoryReservation,
    schema: &[(String, DataType)],
    id: &str,
    payload: &serde_json::Value,
) -> Result<(), QueryError> {
    for (column, (name, data_type)) in columns.iter_mut().zip(schema) {
        let value = if is_row_identity_column(name) {
            Value::String(id.to_owned())
        } else {
            payload
                .as_object()
                .and_then(|object| projected_field_value(object, name))
                .map_or(Ok(Value::Null), |json| {
                    try_json_to_typed_value(json, data_type)
                })?
        };
        memory.try_grow(value_clone_bytes(&value)?)?;
        column.push(value);
    }
    Ok(())
}

#[cfg(test)]
#[path = "authoritative_tests.rs"]
mod tests;
