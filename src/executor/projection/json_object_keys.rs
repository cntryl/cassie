//! Bounded SELECT-list expansion for the selected JSON object-key functions.
use super::{Batch, BatchRow, QueryError};
use crate::executor::batch::chunk_rows;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::SelectItem;
use crate::types::Value;
use std::sync::Arc;

pub(super) fn expand(
    batches: Vec<Batch>,
    projection: &[SelectItem],
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let Some((function, alias)) = object_keys_projection(projection) else {
        return Ok(batches);
    };
    let output_name = alias.unwrap_or(function.name.as_str()).to_owned();
    let mut expanded = AccountedVec::try_new(controls)?;
    for batch in batches {
        for row in batch {
            controls.check_interruption()?;
            match row.get(&output_name) {
                None | Some(Value::Null) => {}
                Some(Value::Json(serde_json::Value::Array(keys))) => {
                    for key in keys {
                        controls.check_interruption()?;
                        let Some(key) = key.as_str() else {
                            return Err(QueryError::General(
                                "json_object_keys produced a non-text key".into(),
                            ));
                        };
                        let retained_bytes = key.len().saturating_add(output_name.len());
                        expanded.try_push_with(retained_bytes, || {
                            BatchRow::from_projected_values(vec![(
                                output_name.clone(),
                                Value::String(key.to_owned()),
                            )])
                            .with_optional_data_types(row.shared_data_types())
                        })?;
                    }
                }
                Some(_) => {
                    return Err(QueryError::General(
                        "json_object_keys projection must produce text rows".into(),
                    ));
                }
            }
        }
    }
    let (mut rows, reservation) = expanded.into_parts();
    let reservation = Arc::new(reservation);
    for row in &mut rows {
        *row = std::mem::replace(row, BatchRow::new(Vec::new()))
            .with_query_memory(Some(reservation.clone()));
    }
    Ok(chunk_rows(rows, crate::executor::batch::DEFAULT_BATCH_SIZE))
}

fn object_keys_projection(
    projection: &[SelectItem],
) -> Option<(&crate::sql::ast::FunctionCall, Option<&str>)> {
    let [item] = projection else { return None };
    let SelectItem::Function { function, alias } = item else {
        return None;
    };
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "json_object_keys" | "jsonb_object_keys"
    )
    .then_some((function, alias.as_deref()))
}
