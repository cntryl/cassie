//! `pg_identify_object(classid, objid, objsubid)` as a FROM source.
use super::{
    batch, finalize_source_batches, graph, BatchRow, QueryError, SourceExecution,
    SourceExecutionEnv, Value,
};
use crate::app::DiagnosticError;
use crate::catalog::object_identity::{ObjectIdentity, ObjectScope};
use crate::types::object_id::{oid_from_integer, parse_oid_text, ObjectIdentifierError};

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    let args = graph::evaluate_args(env, function, outer_row)?;
    let [class_id, object_id, sub_id] = args.as_slice() else {
        return Err(wrong_argument_type());
    };
    let identity = identify(
        env,
        [oid_argument(class_id)?, oid_argument(object_id)?],
        sub_id,
    )?;
    let row = BatchRow::new(vec![
        ("type".to_string(), text(identity.object_type)),
        ("schema".to_string(), text(identity.schema)),
        ("name".to_string(), text(identity.name)),
        ("identity".to_string(), text(identity.identity)),
    ]);
    finalize_source_batches(
        env,
        batch::chunk_rows(vec![row], batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
        qualify,
        name,
    )
}

/// Identifies the object, or returns the all-`NULL` row a strict function
/// gives for a `NULL` argument.
fn identify(
    env: &SourceExecutionEnv<'_>,
    [class_id, object_id]: [Option<u32>; 2],
    sub_id: &Value,
) -> Result<ObjectIdentity, QueryError> {
    let (Some(class_id), Some(object_id), Some(sub_id)) =
        (class_id, object_id, subobject_argument(sub_id)?)
    else {
        return Ok(ObjectIdentity::null());
    };
    let (catalog, context) = env
        .session
        .and_then(crate::app::CassieSession::object_identifier_lookup_context)
        .ok_or_else(|| {
            QueryError::General("object identifier lookup requires a session catalog".to_string())
        })?;
    let table_column = |canonical: &str, attnum: i32| {
        let schema = env.cassie.midge.row_schema(canonical).ok()?;
        schema
            .fields
            .iter()
            .find(|field| !field.retired && i64::from(field.field_id) == i64::from(attnum))
            .map(|field| field.name.clone())
    };
    ObjectScope::new(&catalog, &context)
        .identify_object(class_id, object_id, sub_id, &table_column)
        .map_err(|unsupported| {
            QueryError::Cassie(
                DiagnosticError::new(
                    "0A000",
                    format!(
                        "pg_identify_object does not support catalog class {}",
                        unsupported.0
                    ),
                )
                .into(),
            )
        })
}

fn text(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

/// An `oid` argument; `None` is SQL `NULL`.
fn oid_argument(value: &Value) -> Result<Option<u32>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(number) => oid_from_integer(*number)
            .map(Some)
            .map_err(|rejected| out_of_range(&rejected.to_string())),
        Value::String(text) => parse_oid_text(text)
            .map(Some)
            .map_err(|error| QueryError::Cassie(error.into())),
        _ => Err(wrong_argument_type()),
    }
}

/// The `integer` sub-object argument; `None` is SQL `NULL`.
fn subobject_argument(value: &Value) -> Result<Option<i32>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(number) => i32::try_from(*number).map(Some).map_err(|_| {
            QueryError::Cassie(DiagnosticError::new("22003", "integer out of range").into())
        }),
        _ => Err(wrong_argument_type()),
    }
}

fn out_of_range(rejected: &str) -> QueryError {
    QueryError::Cassie(ObjectIdentifierError::OutOfRange(rejected.to_string()).into())
}

fn wrong_argument_type() -> QueryError {
    QueryError::Cassie(
        DiagnosticError::new(
            "42883",
            "function pg_identify_object requires oid, oid and integer arguments".to_string(),
        )
        .into(),
    )
}
