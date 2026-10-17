//! Size selected SQL datums by declared identity, never by the shared Rust carrier.
use super::{EvalContext, FunctionCall, QueryError, Value};
use crate::app::CassieError;
use crate::executor::batch::RowAccess;
use crate::sql::functions::{datum_size_kind, DatumSizeKind};

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let [argument] = function.args.as_slice() else {
        return Err(
            CassieError::Execution("function pg_column_size(...) does not exist".into()).into(),
        );
    };
    let kind = datum_size_kind(argument)?;
    let value = super::super::evaluate_expr_value_with_context(row, argument, context)?;
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let bytes = match kind {
        DatumSizeKind::Fixed(bytes) => bytes,
        DatumSizeKind::Text | DatumSizeKind::Unknown => {
            let Value::String(text) = value else {
                return Err(carrier());
            };
            text.len()
                .checked_add(if matches!(kind, DatumSizeKind::Text) {
                    4
                } else {
                    1
                })
                .ok_or_else(limit)?
        }
        DatumSizeKind::Bytea => {
            let Value::String(text) = value else {
                return Err(carrier());
            };
            let hex = text
                .strip_prefix("\\x")
                .filter(|hex| {
                    hex.len().is_multiple_of(2) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or_else(carrier)?;
            (hex.len() / 2).checked_add(4).ok_or_else(limit)?
        }
    };
    let bytes = i32::try_from(bytes).map_err(|_| limit())?;
    Ok(Value::Int64(i64::from(bytes)))
}
fn limit() -> QueryError {
    CassieError::ResourceLimit("datum size exceeds the selected int4 result range".into()).into()
}
fn carrier() -> QueryError {
    CassieError::Execution("invalid carrier for the selected pg_column_size argument".into()).into()
}
