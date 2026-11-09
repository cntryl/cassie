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
    let controls = context.controls.or_else(|| {
        context
            .session
            .and_then(crate::app::CassieSession::statement_execution_controls)
    });
    if *op == BinaryOp::Concat {
        if let Some(result) = super::array_operators::concatenate_operand(
            &left_value,
            &right_value,
            operand_type(left),
            operand_type(right),
        ) {
            return result;
        }
    }
    if let BinaryOp::JsonExtract {
        path,
        as_text,
        jsonb,
    } = op
    {
        let document = json_concat::as_json_document(&left_value)?;
        if let (Some(controls), Some(owner)) = (controls, context.deletion_memory) {
            if let Some(value) = json_delete::controlled_extraction(
                &document,
                &right_value,
                *path,
                *as_text,
                *jsonb,
                controls,
                owner,
            )? {
                return Ok(value);
            }
        }
        return json_extract::evaluate(&document, &right_value, *path, *as_text, *jsonb);
    }
    if matches!(op, BinaryOp::Sub | BinaryOp::JsonbDeletePath)
        && (is_json_typed_input(row, left, &left_value)
            || (*op == BinaryOp::Sub
                && matches!(&left_value, ScalarValue::Json(_))
                && !matches!(&right_value, ScalarValue::Array(_))))
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
        if let (Some(controls), Some(owner)) = (controls, context.deletion_memory) {
            let result = if matches!(op, BinaryOp::JsonbDeletePath) {
                json_delete::controlled_path(&left_value, &right_value, controls, owner)?
            } else {
                json_delete::controlled_operator(&left_value, &right_value, controls, owner)?
            };
            if let Some(value) = result {
                return Ok(value);
            }
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
        let left_value = if is_json_typed_input(row, left, &left_value)
            && !matches!(left_value, ScalarValue::Json(_) | ScalarValue::Null)
        {
            json_concat::as_json_document(&left_value)?
        } else {
            left_value
        };
        let right_value = if is_json_typed_input(row, right, &right_value)
            && !matches!(right_value, ScalarValue::Json(_) | ScalarValue::Null)
        {
            json_concat::as_json_document(&right_value)?
        } else {
            right_value
        };
        return json_containment::evaluate_with_controls(op, &left_value, &right_value, controls);
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
    if let Some(result) = array_comparison(row, left, right, op, &left_value, &right_value, context)
    {
        return Ok(result);
    }
    super::binary::binary_scalar_with_controls(&left_value, op, &right_value, controls)
}

fn array_comparison<R: RowAccess + ?Sized>(
    row: &R,
    left: &Expr,
    right: &Expr,
    op: &BinaryOp,
    left_value: &ScalarValue,
    right_value: &ScalarValue,
    context: EvalContext<'_>,
) -> Option<ScalarValue> {
    if matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
    ) {
        if let Some(ordering) = super::super::array_order::compare(
            row,
            (left, &left_value.to_value()),
            (right, &right_value.to_value()),
            context.user_functions,
            context.local_args,
        ) {
            return Some(ScalarValue::Bool(match op {
                BinaryOp::Eq | BinaryOp::IsNotDistinctFrom => ordering.is_eq(),
                BinaryOp::NotEq | BinaryOp::IsDistinctFrom => ordering.is_ne(),
                BinaryOp::Lt => ordering.is_lt(),
                BinaryOp::Lte => !ordering.is_gt(),
                BinaryOp::Gt => ordering.is_gt(),
                BinaryOp::Gte => !ordering.is_lt(),
                _ => unreachable!("only comparison operators reach this branch"),
            }));
        }
    }
    None
}

fn operand_type(expr: &Expr) -> (bool, bool) {
    match expr {
        Expr::Cast { data_type, .. } => {
            if matches!(data_type, crate::types::DataType::Array(_)) {
                (true, false)
            } else {
                (false, true)
            }
        }
        _ => (false, false),
    }
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
