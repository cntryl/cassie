//! Bounded JSONB rows for the selected `jsonb_path_query` table function.
use super::{BatchRow, Expr, QueryError, SourceExecutionEnv, Value};
use crate::runtime::{accounted::AccountedVec, QueryMemoryReservation};
use crate::sql::ast::FunctionCall;

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    outer_row: Option<&BatchRow>,
) -> Result<(Vec<BatchRow>, QueryMemoryReservation), QueryError> {
    env.controls.check_interruption()?;
    let query_name = if function.name.eq_ignore_ascii_case("jsonb_path_query_tz") {
        "jsonb_path_query_array_tz"
    } else {
        "jsonb_path_query_array"
    };
    let call = FunctionCall {
        name: query_name.into(),
        args: function.args.clone(),
        ..FunctionCall::default()
    };
    let empty = BatchRow::new(Vec::new());
    let row = outer_row.unwrap_or(&empty);
    let value = crate::executor::filter::evaluate_expr_value(
        row,
        &Expr::Function(Box::new(call)),
        env.params,
        None,
        env.user_functions,
        env.session,
        None,
    )?;
    let Value::Json(serde_json::Value::Array(values)) = value else {
        return Err(QueryError::General(
            "jsonb_path_query must produce a JSONB array".into(),
        ));
    };
    let mut rows = AccountedVec::try_new(env.controls)?;
    for value in values {
        env.controls.check_interruption()?;
        let bytes = crate::executor::retained_memory::serialized_json_bytes(&value)?
            .saturating_add("value".len())
            .saturating_add(std::mem::size_of::<BatchRow>());
        rows.try_push_with(bytes, || {
            BatchRow::new(vec![("value".into(), Value::Json(value))])
        })?;
    }
    env.controls.check_interruption()?;
    Ok(rows.into_parts())
}
