//! Shared admitted semantic-key conversion for scalar SORT and WINDOW.
use std::collections::HashMap;

use crate::catalog::FunctionMeta;
use crate::executor::batch::RowAccess;
use crate::executor::semantic::SemanticValue;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::Expr;
use crate::types::Value;

use super::accounting;

struct PendingValue {
    value: Value,
    memory: QueryMemoryReservation,
}

pub(crate) fn convert<R: RowAccess>(
    row: &R,
    expression: &Expr,
    value: Value,
    functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
    before_key: impl FnOnce() -> Result<(), QueryError>,
) -> Result<(SemanticValue, QueryMemoryReservation), QueryError> {
    let scratch = accounting::type_scratch(row, expression, functions)?;
    let memory = controls.reserve_query_memory(scratch)?;
    let mut pending = PendingValue { value, memory };
    let data_type = crate::executor::array_order::expression_type(row, expression, functions);
    let mut admitted = accounting::conversion_bytes(&pending.value, data_type.as_ref())?;
    let mut vector_type = None;
    let mut numeric_type = None;
    let mut bit_type = None;
    if let Some(extra) = crate::executor::array_order::sort_vector::bound(
        &pending.value,
        data_type.as_ref(),
        controls,
    )? {
        admitted = crate::executor::retained_memory::add(admitted, extra)?;
        vector_type = data_type;
    } else if let Some(extra) =
        crate::executor::array_order::sort_numeric::bound(&pending.value, data_type.as_ref())?
    {
        admitted = crate::executor::retained_memory::add(admitted, extra)?;
        numeric_type = data_type;
    } else if crate::executor::array_order::sort_bit::eligible(&pending.value, data_type.as_ref()) {
        bit_type = data_type;
    }
    pending.memory.try_grow(admitted)?;
    before_key()?;
    let (semantic, known_retained) = if let Some(data_type) = vector_type.as_ref() {
        crate::executor::array_order::sort_vector_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else if let Some(data_type) = numeric_type.as_ref() {
        crate::executor::array_order::sort_numeric_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else if let Some(data_type) = bit_type.as_ref() {
        crate::executor::array_order::sort_bit_key(
            row,
            expression,
            &pending.value,
            functions,
            data_type,
            controls,
        )?
    } else {
        (
            crate::executor::array_order::key(
                row,
                expression,
                &pending.value,
                functions,
                Some(controls),
            )?,
            None,
        )
    };
    let retained = match known_retained {
        Some(retained) => retained,
        None => accounting::semantic_heap(&semantic)?,
    };
    if retained > admitted {
        return Err(crate::app::CassieError::ResourceLimit(
            "sort key exceeded its admitted allocation shape".into(),
        )
        .into());
    }
    let PendingValue { value, mut memory } = pending;
    drop(value);
    memory.shrink_to(retained);
    Ok((semantic, memory))
}
