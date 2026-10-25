//! Typed JSON and scalar binary expression dispatch.
use super::{
    binary_scalar, constants, eval_scalar_with_context, json_concat, json_containment, json_delete,
    json_extract, temporal, BinaryOp, EvalContext, Expr, QueryError, RowAccess, ScalarValue,
};

pub(super) fn eval_binary_expr<R: RowAccess + ?Sized>(
    row: &R,
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        if let Some(value) = constants::logical(left, op, right, row, context)? {
            return Ok(value);
        }
        let left = eval_scalar_with_context(row, left, context)?;
        if matches!(&left, ScalarValue::Bool(value) if *value == matches!(op, BinaryOp::Or)) {
            return Ok(left);
        }
        let right = eval_scalar_with_context(row, right, context)?;
        return binary_scalar(&left, op, &right);
    }
    let left_value = eval_scalar_with_context(row, left, context)?;
    let right_value = eval_scalar_with_context(row, right, context)?;
    if let BinaryOp::JsonExtract {
        path,
        as_text,
        jsonb,
    } = op
    {
        let document = json_concat::as_json_document(&left_value)?;
        return json_extract::evaluate(&document, &right_value, *path, *as_text, *jsonb);
    }
    if matches!(op, BinaryOp::Sub | BinaryOp::JsonbDeletePath)
        && is_json_typed_input(row, left, &left_value)
    {
        if matches!(&left_value, ScalarValue::Null) || matches!(&right_value, ScalarValue::Null) {
            return Ok(ScalarValue::Null);
        }
        if !matches!(&left_value, ScalarValue::Json(_)) {
            return Err(crate::app::CassieError::InvalidParameterValue(
                "cannot delete from scalar".into(),
            )
            .into());
        }
        return if matches!(op, BinaryOp::JsonbDeletePath) {
            json_delete::delete_path(&left_value, &right_value)
        } else {
            json_delete::delete_operator(&left_value, &right_value)
        };
    }
    if json_containment::handles(op)
        && (is_json_typed_input(row, left, &left_value)
            || is_json_typed_input(row, right, &right_value))
    {
        let left_value = if is_json_typed_input(row, left, &left_value) {
            json_concat::as_json_document(&left_value)?
        } else {
            left_value
        };
        let right_value = if is_json_typed_input(row, right, &right_value) {
            json_concat::as_json_document(&right_value)?
        } else {
            right_value
        };
        return json_containment::evaluate(op, &left_value, &right_value);
    }
    if *op == BinaryOp::Concat {
        let left_is_json = is_json_typed_input(row, left, &left_value);
        let right_is_json = is_json_typed_input(row, right, &right_value);
        if left_is_json || right_is_json {
            let left_value = if left_is_json {
                json_concat::as_json_document(&left_value)?
            } else {
                left_value
            };
            let right_value = if right_is_json {
                json_concat::as_json_document(&right_value)?
            } else {
                right_value
            };
            return json_concat::concatenate(&left_value, &right_value);
        }
    }
    if let Some(result) = temporal::binary(&left_value, op, &right_value, context) {
        return result;
    }
    if matches!(
        op,
        BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    ) {
        if let Some(ordering) = super::super::array_order::compare(
            row,
            (left, &left_value.to_value()),
            (right, &right_value.to_value()),
            context.user_functions,
            context.local_args,
        ) {
            return Ok(ScalarValue::Bool(match op {
                BinaryOp::Lt => ordering.is_lt(),
                BinaryOp::Lte => !ordering.is_gt(),
                BinaryOp::Gt => ordering.is_gt(),
                _ => !ordering.is_lt(),
            }));
        }
    }
    binary_scalar(&left_value, op, &right_value)
}

fn is_json_typed_input<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    value: &ScalarValue,
) -> bool {
    match expression {
        Expr::Column(name) => row
            .column_type(name)
            .is_some_and(crate::types::DataType::is_json),
        Expr::Cast { data_type, .. } if data_type.is_json() => true,
        _ => matches!(value, ScalarValue::Json(_)),
    }
}
