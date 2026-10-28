//! Semantic constants used by AND/OR pruning; dead branches stay dead.
use super::{
    binary_scalar, eval_scalar_with_context, EvalContext, Expr, QueryError, RowAccess, ScalarValue,
};
use crate::sql::ast::{BinaryOp, FunctionCall};
use crate::sql::functions::{function, is_immutable_scalar, FunctionId};

type CastAdmission<'a> = dyn Fn(&Expr) -> Result<(), QueryError> + 'a;

type ParameterResolver<'a> = dyn Fn(usize) -> Result<Option<ScalarValue>, QueryError> + 'a;

#[derive(Clone, Copy)]
struct ConstantContext<'a> {
    evaluation: EvalContext<'a>,
    parameter: Option<&'a ParameterResolver<'a>>,
    admit_cast: Option<&'a CastAdmission<'a>>,
    controls: Option<&'a crate::runtime::QueryExecutionControls>,
}

pub(super) fn value<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    resolve(
        expr,
        row,
        ConstantContext {
            evaluation: context,
            parameter: None,
            admit_cast: None,
            controls: None,
        },
    )
}

pub(super) fn logical<R: RowAccess + ?Sized>(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    resolved_logical(
        left,
        op,
        right,
        row,
        ConstantContext {
            evaluation: context,
            parameter: None,
            admit_cast: None,
            controls: None,
        },
    )
}

fn cast_constant<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let Expr::Cast {
        expr: child,
        data_type,
        source_type,
        identity,
    } = expr
    else {
        unreachable!("only casts use constant conversion");
    };
    if let Some(admit) = context.admit_cast {
        admit(expr)?;
    }
    let controls = context
        .controls
        .or(context.evaluation.controls)
        .or_else(|| {
            context
                .evaluation
                .session
                .and_then(crate::app::CassieSession::statement_execution_controls)
        });
    if let Expr::NumericLiteral(input) = child.as_ref() {
        return super::casts::numeric_literal(input, data_type, *identity, controls).map(Some);
    }
    resolve(child, row, context)?
        .map(|value| super::casts::scalar(&value, data_type, source_type.as_ref(), controls))
        .transpose()
}

fn controls_of(context: ConstantContext<'_>) -> Option<&crate::runtime::QueryExecutionControls> {
    context
        .controls
        .or(context.evaluation.controls)
        .or_else(|| {
            context
                .evaluation
                .session
                .and_then(crate::app::CassieSession::statement_execution_controls)
        })
}

fn resolve<R: RowAccess + ?Sized>(
    expr: &Expr,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    if let Some(session) = context.evaluation.session {
        session.check_statement_interruption()?;
    }
    if let Some(controls) = context.controls {
        controls.check_interruption()?;
    }
    match expr {
        Expr::Column(_) | Expr::Exists(_) | Expr::Subquery(_) => Ok(None),
        Expr::Param(index) => context
            .parameter
            .map_or(Ok(None), |resolve| resolve(*index)),
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::NumericLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => eval_scalar_with_context(row, expr, context.evaluation).map(Some),
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
        } => resolved_logical(left, op, right, row, context),
        Expr::Binary { left, op, right } => {
            match (resolve(left, row, context)?, resolve(right, row, context)?) {
                (Some(left), Some(right)) => {
                    super::temporal::binary_or_scalar(&left, op, &right, controls_of(context))
                        .map(Some)
                }
                _ => Ok(None),
            }
        }
        Expr::Cast { .. } => cast_constant(expr, row, context),
        Expr::Collate { expr, .. } => resolve(expr, row, context),
        Expr::UnaryNumeric { expr, op } => resolve(expr, row, context)?
            .map(|value| {
                super::temporal::negate(&value)
                    .unwrap_or_else(|| super::unary_numeric::evaluate(&value, *op))
            })
            .transpose(),
        Expr::IntegerNegate { expr, width } => resolve(expr, row, context)?
            .map(|value| super::integers::negate(&value, *width))
            .transpose(),
        Expr::Not { expr } => resolve(expr, row, context)?
            .map(|value| {
                value
                    .predicate_result()
                    .map(|value| value.not().into_scalar())
            })
            .transpose(),
        Expr::TruthTest {
            expr,
            truth,
            negated,
        } => resolve(expr, row, context)?
            .map(|value| super::truth_tests::evaluate(&value, *truth, *negated))
            .transpose(),
        Expr::IsNull { expr, negated } => Ok(resolve(expr, row, context)?
            .map(|value| ScalarValue::Bool(matches!(value, ScalarValue::Null) != *negated))),
        expression @ Expr::Like { .. } => like_value(expression, row, context),
        expression @ Expr::Between { .. } => between_predicate(expression, row, context),
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
                super::row_predicates::constant_decision(row, comparison, context.evaluation)?
            {
                return Ok(Some(ScalarValue::Bool(decided)));
            }
            rebuilt(expr, row, context)
        }
        Expr::InList { .. } | Expr::ArraySubscript { .. } => rebuilt(expr, row, context),
    }
}

fn resolved_logical<R: RowAccess + ?Sized>(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    row: &R,
    context: ConstantContext<'_>,
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
            eval_scalar_with_context(row, operand, context.evaluation)?.predicate_result()?;
        }
    }
    let is_or = matches!(op, BinaryOp::Or);
    let left = resolve(left, row, context)?;
    if let Some(left) = &left {
        left.predicate_result()?;
    }
    if matches!(&left, Some(ScalarValue::Bool(value)) if *value == is_or) {
        return Ok(left);
    }
    let right = resolve(right, row, context)?;
    if let Some(right) = &right {
        right.predicate_result()?;
    }
    if matches!(&right, Some(ScalarValue::Bool(value)) if *value == is_or) {
        return Ok(right);
    }
    match (left, right) {
        (Some(left), Some(right)) => {
            super::temporal::binary_or_scalar(&left, op, &right, controls_of(context)).map(Some)
        }
        _ => Ok(None),
    }
}

fn case<R: RowAccess + ?Sized>(
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let operand = match operand {
        Some(expr) => {
            let Some(value) = resolve(expr, row, context)? else {
                return Ok(None);
            };
            Some(value)
        }
        None => None,
    };
    for (when, then) in branches {
        let Some(condition) = resolve(when, row, context)? else {
            return Ok(None);
        };
        let condition = if let Some(operand) = &operand {
            binary_scalar(operand, &BinaryOp::Eq, &condition)?
        } else {
            condition
        };
        if condition.predicate_result()?.is_true() {
            return resolve(then, row, context);
        }
    }
    else_expr.map_or(Ok(Some(ScalarValue::Null)), |expr| {
        resolve(expr, row, context)
    })
}

fn immutable<R: RowAccess + ?Sized>(
    expr: &Expr,
    call: &FunctionCall,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let Some(metadata) = function(&call.name).filter(|metadata| is_immutable_scalar(metadata.id))
    else {
        return Ok(None);
    };
    if metadata.id == FunctionId::Coalesce {
        for argument in &call.args {
            match resolve(argument, row, context)? {
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
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let mut children = Vec::new();
    let mut constant = true;
    expr.try_visit_children(|child| {
        match resolve(child, row, context)? {
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
    eval_scalar_with_context(
        row,
        &expr,
        EvalContext {
            controls: context.controls.or(context.evaluation.controls),
            ..context.evaluation
        },
    )
    .map(Some)
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
        ScalarValue::Typed(value) => Expr::Cast {
            expr: Box::new(Expr::StringLiteral(value.text())),
            data_type: crate::types::DataType::Extended(value.kind()),
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        },
        ScalarValue::Array(value) => Expr::Function(Box::new(FunctionCall {
            name: "_cassie_array_value".to_string(),
            args: vec![Expr::StringLiteral(value.to_storage_json().to_string())],
            quantifier: crate::sql::ast::AggregateQuantifier::Default,
            order_by: Vec::new(),
            filter: None,
        })),
    }
}

fn like_value<R: RowAccess + ?Sized>(
    expression: &Expr,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let Expr::Like {
        expr,
        pattern,
        escape,
        case_insensitive,
    } = expression
    else {
        unreachable!("LIKE constant dispatch")
    };
    let value = resolve(expr, row, context)?;
    let pattern = resolve(pattern, row, context)?;
    let escape = resolve(escape, row, context)?;
    if let (Some(pattern), Some(escape)) = (&pattern, &escape) {
        super::like_predicate::validate_rhs(pattern, escape)?;
    }
    if [&value, &pattern, &escape]
        .iter()
        .any(|value| matches!(value, Some(ScalarValue::Null)))
    {
        return Ok(Some(ScalarValue::Null));
    }
    match (value, pattern, escape) {
        (Some(value), Some(pattern), Some(escape)) => {
            super::like_predicate::scalar(&value, &pattern, &escape, *case_insensitive, || {
                if let Some(controls) = context.controls {
                    controls.check_interruption()?;
                }
                if let Some(session) = context.evaluation.session {
                    session.check_statement_interruption()?;
                }
                Ok(())
            })
            .map(Some)
        }
        _ => Ok(None),
    }
}

fn between_predicate<R: RowAccess + ?Sized>(
    expression: &Expr,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    use super::symmetric_between::{self, Comparison};
    let Expr::Between {
        expr,
        low,
        high,
        negated,
        symmetric,
    } = expression
    else {
        unreachable!("BETWEEN dispatch")
    };
    if !symmetric {
        return between(expr, low, high, *negated, row, context);
    }
    symmetric_between::evaluate(expr, low, high, *negated, |left, op, right| {
        let left = resolve(left, row, context)?;
        let right = resolve(right, row, context)?;
        match (left, right) {
            (Some(left), Some(right)) => {
                symmetric_between::truth(binary_scalar(&left, &op, &right)?).map(Comparison::Known)
            }
            _ => Ok(Comparison::Variable),
        }
    })
    .map(|value| match value {
        Comparison::Known(value) => Some(symmetric_between::scalar(value)),
        Comparison::Variable => None,
    })
}

fn between<R: RowAccess + ?Sized>(
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    row: &R,
    context: ConstantContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let operand = resolve(expr, row, context)?;
    let low = resolve(low, row, context)?;
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
    let high = resolve(high, row, context)?;
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

pub(crate) fn constant_scalar_with_parameters(
    expression: &Expr,
    parameters: &[super::Value],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(Option<ScalarValue>, crate::runtime::QueryMemoryReservation), QueryError> {
    use crate::executor::retained_memory::{mul, value_clone_bytes};
    let memory = std::cell::RefCell::new(controls.reserve_query_memory(0)?);
    let parameter = |index| {
        controls.check_interruption()?;
        let Some(value) = parameters.get(index) else {
            return Ok(None);
        };
        if matches!(value, super::Value::Json(_) | super::Value::Vector(_)) {
            return Ok(None);
        }
        // The lazy semantic walk calls this only for reached parameters.
        // Reserve before copying into scalar/rewritten constant intermediates.
        memory
            .borrow_mut()
            .try_grow(mul(value_clone_bytes(value)?, 4)?)?;
        Ok(Some(super::scalar_from_value(value)))
    };
    let admit_cast = |expr: &Expr| {
        controls.check_interruption()?;
        memory
            .borrow_mut()
            .try_grow(super::casts::bytea_literal_scratch(expr)?)?;
        Ok(())
    };
    let value = evaluate_constant(
        expression,
        parameters,
        Some(&parameter),
        Some(&admit_cast),
        Some(controls),
    )?;
    Ok((value, memory.into_inner()))
}

fn evaluate_constant(
    expression: &Expr,
    parameters: &[super::Value],
    parameter: Option<&ParameterResolver<'_>>,
    admit_cast: Option<&CastAdmission<'_>>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Option<ScalarValue>, QueryError> {
    let functions = super::HashMap::new();
    let row: Vec<(String, super::Value)> = Vec::new();
    resolve(
        expression,
        &row,
        ConstantContext {
            evaluation: EvalContext {
                params: parameters,
                search_context: None,
                user_functions: &functions,
                local_args: None,
                session: None,
                exists: None,
                controls,
            },
            parameter,
            admit_cast,
            controls,
        },
    )
}
