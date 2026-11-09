//! Keeps source row owners alive through a materialized projection batch write.
use std::sync::Arc;

use crate::executor::batch::{BatchRow, OperatorMemory};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Schema;

use super::QueryError;

#[cfg(test)]
#[path = "materialized_projection_payload_tests.rs"]
mod tests;

type Document = (String, serde_json::Value);

struct SourceOwners {
    _query: Option<Arc<QueryMemoryReservation>>,
    _operator: Option<Arc<OperatorMemory>>,
}

pub(super) struct OwnedProjectionDocuments {
    documents: Vec<Document>,
    owners: Vec<SourceOwners>,
    _directory_memory: QueryMemoryReservation,
}

pub(super) fn encode(
    rows: Vec<BatchRow>,
    schema: &Schema,
    controls: &QueryExecutionControls,
) -> Result<OwnedProjectionDocuments, QueryError> {
    encode_observing(rows, schema, controls, |_, _| {})
}

fn encode_observing(
    rows: Vec<BatchRow>,
    schema: &Schema,
    controls: &QueryExecutionControls,
    mut observe_field: impl FnMut(usize, usize),
) -> Result<OwnedProjectionDocuments, QueryError> {
    use crate::executor::retained_memory::{add, mul};
    let count = rows.len();
    let bytes = add(
        mul(count, std::mem::size_of::<Document>())?,
        mul(count, std::mem::size_of::<SourceOwners>())?,
    )?;
    let memory = controls.reserve_query_memory(bytes)?;
    let mut output = OwnedProjectionDocuments {
        documents: Vec::new(),
        owners: Vec::new(),
        _directory_memory: memory,
    };
    reserve_exact(&mut output.documents, count)?;
    reserve_exact(&mut output.owners, count)?;
    for (index, row) in rows.into_iter().enumerate() {
        let query = row.query_memory();
        let operator = row.operator_memory();
        let entries = row.into_entries();
        if entries.len() != schema.fields.len() {
            return Err(QueryError::General(format!(
                "materialized projection produced {} values for {} output columns",
                entries.len(),
                schema.fields.len()
            )));
        }
        let mut payload = serde_json::Map::new();
        for (field_index, ((_, value), field)) in
            entries.into_iter().zip(&schema.fields).enumerate()
        {
            let value = crate::types::json::try_value_to_json(value).map_err(|error| {
                QueryError::General(format!("materialized projection output {error}"))
            })?;
            payload.insert(field.name.clone(), value);
            observe_field(index, field_index);
            super::super::check_timeout(controls)?;
        }
        let payload = serde_json::Value::Object(payload);
        let id = super::deterministic_row_id(index, &payload);
        output.documents.push((id, payload));
        output.owners.push(SourceOwners {
            _query: query,
            _operator: operator,
        });
    }
    Ok(output)
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<(), QueryError> {
    values
        .try_reserve_exact(count)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    if values.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "materialized projection directory allocation exceeded its admission".into(),
        )
        .into());
    }
    Ok(())
}

impl OwnedProjectionDocuments {
    pub(super) fn publish_with<T>(
        &mut self,
        publish: impl FnOnce(Vec<Document>) -> Result<T, QueryError>,
    ) -> Result<T, QueryError> {
        publish(std::mem::take(&mut self.documents))
    }
}
