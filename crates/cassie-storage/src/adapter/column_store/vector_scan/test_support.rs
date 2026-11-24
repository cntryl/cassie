//! Narrow Engine test access to the concrete vector scan's result carriers.

use super::{ColumnVectorBatch, ColumnVectorOutput};
use crate::adapter::{
    CassieError, Midge, QueryMemoryReservation, RowDecode, RowTraversal, StorageRequestContext,
};
use crate::row_blob::RowSchema;
use cassie_core::types::lanes::RawLanes;
use cassie_core::types::{FieldSchema, Schema};

/// One vector batch retained with its original query-memory lease.
#[derive(Debug)]
pub struct TestColumnVectorBatch {
    pub ids: Vec<String>,
    pub lanes: Vec<RawLanes>,
    _memory: QueryMemoryReservation,
}

/// Output binding for one requested vector-reader column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestColumnVectorOutput {
    Identity,
    Lane(usize),
}

fn retain_batch(batch: ColumnVectorBatch) -> TestColumnVectorBatch {
    TestColumnVectorBatch {
        ids: batch.ids,
        lanes: batch.lanes,
        _memory: batch.memory,
    }
}

/// Reads all selected column lanes through the production native reader.
pub fn read_column_vector_batches(
    midge: &Midge,
    collection: &str,
    requested: &[Option<&str>],
    complete_source: bool,
    controls: &StorageRequestContext<'_>,
) -> Result<Option<Vec<TestColumnVectorBatch>>, CassieError> {
    let traversal = if complete_source {
        RowTraversal::CompleteSource
    } else {
        RowTraversal::VisitedRows
    };
    let Some(mut reader) = midge.open_column_vector_reader(collection, requested, traversal)?
    else {
        return Ok(None);
    };
    let mut batches = Vec::new();
    while let Some(batch) = reader.next_batch(midge, super::COLUMN_VECTOR_BATCH_ROWS, controls)? {
        batches.push(retain_batch(batch));
    }
    Ok(Some(batches))
}

/// Returns the result binding for each requested vector-reader column.
pub fn column_vector_outputs(
    midge: &Midge,
    collection: &str,
    requested: &[Option<&str>],
    complete_source: bool,
) -> Result<Option<Vec<TestColumnVectorOutput>>, CassieError> {
    let traversal = if complete_source {
        RowTraversal::CompleteSource
    } else {
        RowTraversal::VisitedRows
    };
    let Some(reader) = midge.open_column_vector_reader(collection, requested, traversal)? else {
        return Ok(None);
    };
    Ok(Some(
        reader
            .outputs()
            .iter()
            .map(|output| match output {
                ColumnVectorOutput::Identity => TestColumnVectorOutput::Identity,
                ColumnVectorOutput::Lane(index) => TestColumnVectorOutput::Lane(*index),
            })
            .collect(),
    ))
}

/// Reads row documents and their declared fields through the production cursor.
pub fn read_row_documents(
    midge: &Midge,
    collection: &str,
    controls: &StorageRequestContext<'_>,
) -> Result<(Schema, Vec<(String, serde_json::Value)>), CassieError> {
    let row_schema: RowSchema = midge.row_schema(collection)?;
    let schema = Schema {
        fields: row_schema
            .fields
            .iter()
            .filter(|field| !field.retired)
            .map(|field| FieldSchema {
                name: field.name.clone(),
                data_type: field.data_type.clone(),
                nullable: field.nullable,
            })
            .collect(),
    };
    let Some(mut cursor) = midge.open_row_cursor(collection, RowDecode::Full)? else {
        return Ok((schema, Vec::new()));
    };
    let mut rows = Vec::new();
    loop {
        let page = cursor.next_documents(midge, 64, controls)?;
        if page.is_empty() {
            break;
        }
        rows.extend(
            page.into_iter()
                .map(|document| (document.id, document.payload)),
        );
    }
    Ok((schema, rows))
}
