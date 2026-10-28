//! Owned candidates survive a unique-owner wait without reevaluating SQL expressions.
use super::{
    Cassie, CassieSession, CollectionSchema, FunctionMeta, HashMap, QueryError,
    QueryExecutionControls, QueryResult, Value,
};
use crate::executor::retained_memory::{add, mul, serialized_json_bytes};
use crate::runtime::accounted::AccountedVec;

pub(in crate::executor::execution) struct WaitingInsert {
    schema: CollectionSchema,
    has_exists_returning: bool,
    returning_read_controls: Option<QueryExecutionControls>,
    payloads: AccountedVec<serde_json::Value>,
}
impl WaitingInsert {
    pub(in crate::executor::execution) fn payloads(&self) -> &[serde_json::Value] {
        self.payloads.as_slice()
    }
}

pub(in crate::executor::execution) fn prepare_waiting_insert(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<WaitingInsert, QueryError> {
    let schema = cassie
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| crate::app::CassieError::CollectionNotFound(statement.table.to_string()))?;
    let (has_exists_returning, returning_read_controls) =
        super::prepare_insert_returning_read_controls(
            cassie,
            Some(session),
            &statement.returning,
            controls,
        )?;
    let (fields, rows) = super::prepare_insert_source_rows(
        cassie,
        Some(session),
        statement,
        params,
        functions,
        &schema,
        returning_read_controls.as_ref().unwrap_or(controls),
    )?;
    let defaults = cassie.catalog.get_constraints(&statement.table);
    let mut extra = 256;
    for constraint in &defaults {
        extra = add(extra, add(constraint.field.len(), 128)?)?;
        if let Some(value) = &constraint.default_value {
            extra = add(extra, serialized_json_bytes(value)?)?;
        }
    }
    let names = fields
        .iter()
        .try_fold(0, |bytes, field| add(bytes, mul(field.name.len(), 6)?))?;
    let mut payloads = AccountedVec::try_new(controls)?;
    for row in rows {
        controls.check_interruption()?;
        let bytes = row
            .iter()
            .filter_map(super::cells::InsertValue::as_value)
            .try_fold(add(extra, names)?, |bytes, value| {
                add(bytes, value_bytes(value)?)
            })?;
        let bound = mul(
            crate::runtime::accounted::json::serialized_decode_bytes(bytes)?,
            2,
        )?;
        payloads.try_push_with_result(bound, || {
            let payload = super::cells::payload(cassie, &statement.table, &fields, &row)
                .map_err(crate::app::CassieError::from)?;
            cassie.prepare_upsert_candidate(&statement.table, payload)
        })?;
    }
    Ok(WaitingInsert {
        schema,
        has_exists_returning,
        returning_read_controls,
        payloads,
    })
}

fn value_bytes(value: &Value) -> Result<usize, crate::app::CassieError> {
    use crate::types::{extended::numeric::Numeric, typed::TypedValue};
    match value {
        Value::String(text) => add(mul(text.len(), 6)?, 16),
        Value::Json(value) => serialized_json_bytes(value),
        Value::Typed(TypedValue::Numeric(Numeric::Finite { mantissa, scale })) => add(
            usize::try_from(mantissa.bits()).unwrap_or(usize::MAX),
            add(*scale as usize, 32)?,
        ),
        Value::Typed(value) => add(mul(value.heap_bytes(), 8)?, 256),
        Value::Vector(value) => add(mul(value.values.len(), 64)?, 16),
        _ => Ok(64),
    }
}

pub(in crate::executor::execution) fn execute_prepared_insert(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &crate::sql::ast::InsertStatement,
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
    prepared: WaitingInsert,
) -> Result<QueryResult, QueryError> {
    let WaitingInsert {
        schema,
        has_exists_returning,
        returning_read_controls,
        payloads,
    } = prepared;
    let (rows, _memory) = payloads.into_parts();
    super::execute_insert_payloads(
        cassie,
        Some(session),
        statement,
        params,
        functions,
        controls,
        super::InsertPayloads {
            schema,
            has_exists_returning,
            returning_read_controls,
            payloads: rows.into_iter().map(Ok),
        },
    )
}
