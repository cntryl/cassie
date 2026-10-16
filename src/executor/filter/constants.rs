//! Semantic constants used by AND/OR pruning; dead branches stay dead.
use super::{
    binary_scalar, eval_scalar_with_context, EvalContext, Expr, QueryError, RowAccess, ScalarValue,
};
use crate::sql::ast::{BinaryOp, FunctionCall};
use crate::sql::functions::{function, is_immutable_scalar, FunctionId};

pub(super) fn value<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    match expr {
        Expr::Column(_) | Expr::Param(_) | Expr::Exists(_) => Ok(None),
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => eval_scalar_with_context(row, expr, context).map(Some),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => case(
            operand.as_deref(),
            branches,
            else_expr.as_deref(),
            row,
            context,
        ),
        Expr::Function(call) => immutable(expr, call, row, context),
        Expr::Binary {
            left,
            op: op @ (BinaryOp::And | BinaryOp::Or),
            right,
        } => logical(left, op, right, row, context),
        Expr::Binary { left, op, right } => {
            match (value(left, row, context)?, value(right, row, context)?) {
                (Some(left), Some(right)) => binary_scalar(&left, op, &right).map(Some),
                _ => Ok(None),
            }
        }
        Expr::Cast {
            expr: child,
            data_type,
            source_type,
            identity,
        } => {
            if let Expr::NumericLiteral(input) = child.as_ref() {
                return super::casts::numeric_literal(input, data_type, *identity).map(Some);
            }
            value(child, row, context)?
                .map(|value| super::casts::scalar(&value, data_type, source_type.as_ref()))
                .transpose()
        }
        Expr::UnaryNumeric { expr, op } => value(expr, row, context)?
            .map(|value| super::unary_numeric::evaluate(&value, *op))
            .transpose(),
        Expr::IntegerNegate { expr, width } => value(expr, row, context)?
            .map(|value| super::integers::negate(&value, *width))
            .transpose(),
        Expr::Not { expr } => value(expr, row, context)?
            .map(|value| {
                value
                    .predicate_result()
                    .map(|value| value.not().into_scalar())
            })
            .transpose(),
        Expr::IsNull { expr, negated } => Ok(value(expr, row, context)?
            .map(|value| ScalarValue::Bool(matches!(value, ScalarValue::Null) != *negated))),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => between(expr, low, high, *negated, row, context),
        Expr::RowCompare(comparison) => {
            if comparison.left.len() > 1
                && !comparison.op.null_safe()
                && !matches!(
                    comparison.op,
                    crate::sql::ast::RowComparisonOp::Eq | crate::sql::ast::RowComparisonOp::NotEq
                )
            {
                return Ok(None);
            }
            if let Some(decided) =
                super::row_predicates::constant_decision(row, comparison, context)?
            {
                return Ok(Some(ScalarValue::Bool(decided)));
            }
            rebuilt(expr, row, context)
        }
        Expr::InList { .. } => rebuilt(expr, row, context),
    }
}

pub(super) fn logical<R: RowAccess + ?Sized>(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    // Binding validates SQL operand types. Keep the same gate for callers
    // that evaluate an AST directly, without evaluating a dead expression.
    for operand in [left, right] {
        if matches!(
            operand,
            Expr::Param(_)
                | Expr::Column(_)
                | Expr::StringLiteral(_)
                | Expr::NumberLiteral(_)
                | Expr::IntegerLiteral(_)
                | Expr::NumericLiteral(_)
        ) {
            eval_scalar_with_context(row, operand, context)?.predicate_result()?;
        }
    }
    let is_or = matches!(op, BinaryOp::Or);
    let left = value(left, row, context)?;
    if let Some(left) = &left {
        left.predicate_result()?;
    }
    if matches!(&left, Some(ScalarValue::Bool(value)) if *value == is_or) {
        return Ok(left);
    }
    let right = value(right, row, context)?;
    if let Some(right) = &right {
        right.predicate_result()?;
    }
    if matches!(&right, Some(ScalarValue::Bool(value)) if *value == is_or) {
        return Ok(right);
    }
    match (left, right) {
        (Some(left), Some(right)) => binary_scalar(&left, op, &right).map(Some),
        _ => Ok(None),
    }
}

fn case<R: RowAccess + ?Sized>(
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let operand = match operand {
        Some(expr) => {
            let Some(value) = value(expr, row, context)? else {
                return Ok(None);
            };
            Some(value)
        }
        None => None,
    };
    for (when, then) in branches {
        let Some(condition) = value(when, row, context)? else {
            return Ok(None);
        };
        let condition = if let Some(operand) = &operand {
            binary_scalar(operand, &BinaryOp::Eq, &condition)?
        } else {
            condition
        };
        if condition.predicate_result()?.is_true() {
            return value(then, row, context);
        }
    }
    else_expr.map_or(Ok(Some(ScalarValue::Null)), |expr| {
        value(expr, row, context)
    })
}

fn immutable<R: RowAccess + ?Sized>(
    expr: &Expr,
    call: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let Some(metadata) = function(&call.name).filter(|metadata| is_immutable_scalar(metadata.id))
    else {
        return Ok(None);
    };
    if metadata.id == FunctionId::Coalesce {
        for argument in &call.args {
            match value(argument, row, context)? {
                Some(ScalarValue::Null) => {}
                result => return Ok(result),
            }
        }
        return Ok(Some(ScalarValue::Null));
    }
    rebuilt(expr, row, context)
}

fn rebuilt<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let mut children = Vec::new();
    let mut constant = true;
    expr.try_visit_children(|child| {
        match value(child, row, context)? {
            Some(value) => children.push(literal(value)),
            None => constant = false,
        }
        Ok::<_, QueryError>(())
    })?;
    if !constant {
        return Ok(None);
    }
    let mut children = children.into_iter();
    let expr = expr.map_children(|_| children.next().expect("one value per constant child"));
    eval_scalar_with_context(row, &expr, context).map(Some)
}

fn literal(value: ScalarValue) -> Expr {
    match value {
        ScalarValue::Null => Expr::Null,
        ScalarValue::Bool(value) => Expr::BoolLiteral(value),
        ScalarValue::Int(value) => Expr::IntegerLiteral(value),
        ScalarValue::Float(value) => Expr::NumberLiteral(value),
        ScalarValue::Str(value) => Expr::StringLiteral(value),
        ScalarValue::Json(value) => Expr::Cast {
            expr: Box::new(Expr::StringLiteral(value)),
            data_type: crate::types::DataType::Json,
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        },
    }
}

fn between<R: RowAccess + ?Sized>(
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let operand = value(expr, row, context)?;
    let low = value(low, row, context)?;
    let first = match (&operand, low) {
        (Some(operand), Some(low)) => Some(binary_scalar(
            operand,
            &if negated { BinaryOp::Lt } else { BinaryOp::Gte },
            &low,
        )?),
        _ => None,
    };
    if matches!(&first, Some(ScalarValue::Bool(result)) if *result == negated) {
        return Ok(first);
    }
    let high = value(high, row, context)?;
    let second = match (&operand, high) {
        (Some(operand), Some(high)) => Some(binary_scalar(
            operand,
            &if negated { BinaryOp::Gt } else { BinaryOp::Lte },
            &high,
        )?),
        _ => None,
    };
    if matches!(&second, Some(ScalarValue::Bool(result)) if *result == negated) {
        return Ok(second);
    }
    match (first, second) {
        (Some(first), Some(second)) => binary_scalar(
            &first,
            &if negated { BinaryOp::Or } else { BinaryOp::And },
            &second,
        )
        .map(Some),
        _ => Ok(None),
    }
}

pub(crate) fn constant_scalar(expression: &Expr) -> Result<Option<ScalarValue>, QueryError> {
    let functions = super::HashMap::new();
    let row: Vec<(String, super::Value)> = Vec::new();
    value(
        expression,
        &row,
        EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
        },
    )
}
