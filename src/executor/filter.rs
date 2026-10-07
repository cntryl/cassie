use crate::executor::batch::RowAccess;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::Batch;
use crate::executor::semantic::compare_numeric_values;
use crate::executor::QueryError;
use crate::search::analyzer::AnalyzerConfig;
use crate::sql::ast::FunctionCall;
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{DataType, Value};
use uuid::Uuid;

/// PostgreSQL's message (SQLSTATE 22003) for an overflowing int8 result.
pub(crate) const BIGINT_OUT_OF_RANGE: &str = "bigint out of range";

#[path = "filter/coalesce.rs"]
mod coalesce;
#[path = "filter/conditional.rs"]
mod conditional;
#[path = "filter/functions.rs"]
mod functions;
#[path = "filter/like.rs"]
mod like;
#[path = "filter/search.rs"]
mod search;

use functions::{evaluate_function, parse_vector_text};
pub(crate) use like::like_matches;
#[cfg(test)]
pub(crate) use search::{prepare_query_terms, SingleFieldSearchContext};
pub(crate) use search::{
    prepare_query_terms_with_analyzer, PersistedFieldStatistics, SearchContext, SearchTermStats,
};

#[derive(Clone, Copy)]
pub(super) struct EvalContext<'a> {
    params: &'a [Value],
    search_context: Option<&'a SearchContext>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    local_args: Option<&'a HashMap<String, Value>>,
    session: Option<&'a CassieSession>,
}

#[derive(Debug, Clone)]
pub(crate) enum ScalarValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Json(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PredicateResult {
    True,
    False,
    Unknown,
}

impl PredicateResult {
    fn and(self, other: Self) -> Self {
        match (self, other) {
            (Self::False, _) | (_, Self::False) => Self::False,
            (Self::True, Self::True) => Self::True,
            _ => Self::Unknown,
        }
    }

    fn or(self, other: Self) -> Self {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Self::True,
            (Self::False, Self::False) => Self::False,
            _ => Self::Unknown,
        }
    }

    const fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    const fn is_true(self) -> bool {
        matches!(self, Self::True)
    }

    const fn into_scalar(self) -> ScalarValue {
        match self {
            Self::True => ScalarValue::Bool(true),
            Self::False => ScalarValue::Bool(false),
            Self::Unknown => ScalarValue::Null,
        }
    }
}

impl ScalarValue {
    pub(crate) fn predicate_result(&self) -> Result<PredicateResult, QueryError> {
        match self {
            ScalarValue::Bool(true) => Ok(PredicateResult::True),
            ScalarValue::Bool(false) => Ok(PredicateResult::False),
            ScalarValue::Null => Ok(PredicateResult::Unknown),
            ScalarValue::Int(_)
            | ScalarValue::Float(_)
            | ScalarValue::Str(_)
            | ScalarValue::Json(_) => Err(QueryError::General(
                "Boolean expression requires BOOLEAN or SQL NULL".to_string(),
            )),
        }
    }

    pub(crate) fn is_true(&self) -> Result<bool, QueryError> {
        self.predicate_result().map(PredicateResult::is_true)
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            ScalarValue::Str(v) | ScalarValue::Json(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn to_f64(&self) -> Option<f64> {
        match self {
            ScalarValue::Float(v) => Some(*v),
            ScalarValue::Int(v) => Some(crate::types::numeric::i64_to_f64(*v)),
            ScalarValue::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            ScalarValue::Null => Value::Null,
            ScalarValue::Bool(v) => Value::Bool(*v),
            ScalarValue::Int(v) => Value::Int64(*v),
            ScalarValue::Float(v) => Value::Float64(*v),
            ScalarValue::Str(v) | ScalarValue::Json(v) => Value::String(v.clone()),
        }
    }
}

pub(crate) fn filter_rows<R>(
    rows: Vec<R>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<R>, QueryError>
where
    R: RowAccess,
{
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if eval_filter(
            &row,
            expression,
            params,
            search_context,
            user_functions,
            session,
        )? {
            out.push(row);
        }
    }
    Ok(out)
}

pub(crate) fn filter_batches(
    batches: Vec<Batch>,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<Vec<Batch>, QueryError> {
    batches
        .into_iter()
        .map(|batch| {
            filter_rows(
                batch,
                expression,
                params,
                search_context,
                user_functions,
                session,
            )
        })
        .collect()
}

pub(crate) fn evaluate_expr_value<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    evaluate_expr_value_with_context(
        row,
        expr,
        EvalContext {
            params,
            search_context,
            user_functions,
            local_args,
            session,
        },
    )
}

fn evaluate_expr_value_with_context<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    match expr {
        Expr::Param(index) => Ok(context.params.get(*index).cloned().unwrap_or(Value::Null)),
        Expr::Column(name) => Ok(column_value_ref(row, name, context.local_args)
            .cloned()
            .unwrap_or(Value::Null)),
        Expr::Function(function) => evaluate_function(function, row, context),
        _ => Ok(eval_scalar_with_context(row, expr, context)?.to_value()),
    }
}

fn eval_filter<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
) -> Result<bool, QueryError> {
    let value = eval_scalar_with_context(
        row,
        expression,
        EvalContext {
            params,
            search_context,
            user_functions,
            local_args: None,
            session,
        },
    )?;
    value.is_true()
}

pub(crate) fn eval_scalar<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    local_args: Option<&HashMap<String, Value>>,
    session: Option<&CassieSession>,
) -> Result<ScalarValue, QueryError> {
    eval_scalar_with_context(
        row,
        expr,
        EvalContext {
            params,
            search_context,
            user_functions,
            local_args,
            session,
        },
    )
}

fn eval_scalar_with_context<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    match expr {
        Expr::Column(name) => Ok(eval_column_value(row, name, context.local_args)),
        Expr::StringLiteral(value) => Ok(ScalarValue::Str(value.clone())),
        Expr::NumberLiteral(value) => Ok(ScalarValue::Float(*value)),
        Expr::IntegerLiteral(value) => Ok(ScalarValue::Int(*value)),
        Expr::BoolLiteral(value) => Ok(ScalarValue::Bool(*value)),
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            let operand_value = operand
                .as_ref()
                .map(|operand| eval_scalar_with_context(row, operand, context))
                .transpose()?;
            for (when, then) in branches {
                let when_value = eval_scalar_with_context(row, when, context)?;
                if operand_value.is_none()
                    && !matches!(when_value, ScalarValue::Bool(_) | ScalarValue::Null)
                {
                    return Err(QueryError::General(
                        "CASE WHEN condition must be BOOLEAN".to_string(),
                    ));
                }
                let selected = if let Some(operand_value) = &operand_value {
                    eq_value(operand_value, &when_value) == Some(true)
                } else {
                    matches!(when_value, ScalarValue::Bool(true))
                };
                if selected {
                    return eval_scalar_with_context(row, then, context);
                }
            }
            else_expr.as_ref().map_or(Ok(ScalarValue::Null), |expr| {
                eval_scalar_with_context(row, expr, context)
            })
        }
        Expr::Param(index) => Ok(context
            .params
            .get(*index)
            .map_or(ScalarValue::Null, scalar_from_value)),
        Expr::Function(function) => evaluate_function_scalar(row, function, context),
        Expr::Binary { left, op, right } => eval_binary_expr(row, left, op, right, context),
        Expr::IsNull { expr, negated } => eval_is_null_expr(row, expr, *negated, context),
        Expr::InList {
            expr,
            values,
            negated,
        } => eval_in_list_expr(row, expr, values, *negated, context),
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => eval_between_expr(row, expr, low, high, *negated, context),
        Expr::Not { expr } => eval_not_expr(row, expr, context),
        Expr::Cast { expr, data_type } => eval_cast_expr(row, expr, data_type, context),
        Expr::Exists(_) => Err(QueryError::General(
            "EXISTS predicate was not resolved before filtering".to_string(),
        )),
    }
}

/// Casts a scalar to DATE/TIME/TIMESTAMP using the shared temporal parser,
/// so a cast validates and canonicalizes the same way storage does instead
/// of passing arbitrary strings through unchanged.
fn cast_temporal_scalar(
    value: &ScalarValue,
    type_name: &str,
    canonicalize: impl Fn(&str) -> Result<String, String>,
) -> Result<ScalarValue, QueryError> {
    let ScalarValue::Str(value) = value else {
        return Err(QueryError::General(format!(
            "cannot cast value to {type_name}"
        )));
    };
    canonicalize(value)
        .map(ScalarValue::Str)
        .map_err(|error| QueryError::General(format!("cannot cast value to {type_name}: {error}")))
}

fn cast_scalar(value: &ScalarValue, data_type: &DataType) -> Result<ScalarValue, QueryError> {
    if matches!(value, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }

    match data_type {
        DataType::Null => Ok(ScalarValue::Null),
        DataType::SmallInt => scalar_to_i64(value)
            .and_then(|value| i16::try_from(value).ok())
            .map(|value| ScalarValue::Int(i64::from(value)))
            .ok_or_else(|| QueryError::General("cannot cast value to SMALLINT".to_string())),
        DataType::Int => scalar_to_i64(value)
            .and_then(|value| i32::try_from(value).ok())
            .map(|value| ScalarValue::Int(i64::from(value)))
            .ok_or_else(|| QueryError::General("cannot cast value to INT".to_string())),
        DataType::BigInt => scalar_to_i64(value)
            .map(ScalarValue::Int)
            .ok_or_else(|| QueryError::General("cannot cast value to BIGINT".to_string())),
        DataType::Float => value
            .to_f64()
            .filter(|value| value.is_finite())
            .map(ScalarValue::Float)
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite())
                    .map(ScalarValue::Float)
            })
            .ok_or_else(|| QueryError::General("cannot cast value to FLOAT".to_string())),
        DataType::Boolean => cast_boolean_scalar(value),
        DataType::Text => Ok(ScalarValue::Str(match value {
            ScalarValue::Bool(value) => value.to_string(),
            ScalarValue::Int(value) => value.to_string(),
            ScalarValue::Float(value) => value.to_string(),
            ScalarValue::Str(value) | ScalarValue::Json(value) => value.clone(),
            ScalarValue::Null => String::new(),
        })),
        DataType::Char { length } => cast_bounded_text(value, length.unwrap_or(1), "CHAR"),
        DataType::Varchar { length } => cast_varchar_text(value, *length),
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to BYTEA".to_string()))?;
            decode_bytea(value)?;
            Ok(ScalarValue::Str(format!(
                "\\x{}",
                value[2..].to_ascii_lowercase()
            )))
        }
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to UUID".to_string()))?;
            let canonical = Uuid::parse_str(value)
                .map_err(|_| QueryError::General("cannot cast value to UUID".to_string()))?;
            Ok(ScalarValue::Str(canonical.to_string()))
        }
        DataType::Date => {
            cast_temporal_scalar(value, "DATE", crate::types::temporal::canonical_date)
        }
        DataType::Time => {
            cast_temporal_scalar(value, "TIME", crate::types::temporal::canonical_time)
        }
        DataType::Timestamp => cast_temporal_scalar(
            value,
            "TIMESTAMP",
            crate::types::temporal::canonical_timestamp,
        ),
        DataType::Json => Ok(ScalarValue::Json(cast_json_text(value))),
        DataType::Array(_) => Err(QueryError::General(
            "cannot cast scalar value to ARRAY".to_string(),
        )),
        DataType::Vector(_) => Err(QueryError::General(
            "cannot cast scalar value to VECTOR".to_string(),
        )),
    }
}

fn cast_to_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Bool(value) => Some(value.to_string()),
        ScalarValue::Int(value) => Some(value.to_string()),
        ScalarValue::Float(value) => Some(value.to_string()),
        ScalarValue::Str(value) | ScalarValue::Json(value) => Some(value.clone()),
        ScalarValue::Null => None,
    }
}

fn cast_boolean_scalar(value: &ScalarValue) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Bool(value) => Ok(ScalarValue::Bool(*value)),
        ScalarValue::Int(value) => Ok(ScalarValue::Bool(*value != 0)),
        ScalarValue::Float(value) => Ok(ScalarValue::Bool(*value != 0.0)),
        ScalarValue::Str(value) | ScalarValue::Json(value) => {
            crate::types::boolean::parse_text(value)
                .map(ScalarValue::Bool)
                .ok_or_else(|| QueryError::General("cannot cast value to BOOLEAN".to_string()))
        }
        ScalarValue::Null => Ok(ScalarValue::Null),
    }
}

fn cast_bounded_text(
    value: &ScalarValue,
    max_length: u32,
    type_name: &str,
) -> Result<ScalarValue, QueryError> {
    let value = cast_to_text(value)
        .filter(|value| {
            usize::try_from(max_length).is_ok_and(|max_length| value.chars().count() <= max_length)
        })
        .ok_or_else(|| QueryError::General(format!("cannot cast value to {type_name}")))?;
    Ok(ScalarValue::Str(value))
}

fn cast_varchar_text(
    value: &ScalarValue,
    max_length: Option<u32>,
) -> Result<ScalarValue, QueryError> {
    let value = cast_to_text(value)
        .filter(|value| {
            max_length.is_none_or(|max_length| {
                usize::try_from(max_length)
                    .is_ok_and(|max_length| value.chars().count() <= max_length)
            })
        })
        .ok_or_else(|| QueryError::General("cannot cast value to VARCHAR".to_string()))?;
    Ok(ScalarValue::Str(value))
}

fn cast_json_text(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Bool(value) => value.to_string(),
        ScalarValue::Int(value) => value.to_string(),
        ScalarValue::Float(value) => value.to_string(),
        ScalarValue::Str(value) | ScalarValue::Json(value) => value.clone(),
        ScalarValue::Null => "null".to_string(),
    }
}

fn scalar_to_i64(value: &ScalarValue) -> Option<i64> {
    match value {
        ScalarValue::Int(value) => Some(*value),
        ScalarValue::Bool(value) => Some(i64::from(*value)),
        // PostgreSQL rounds float-to-integer casts to the nearest integer
        // (ties to even, like `rint`) and fails only on range overflow.
        ScalarValue::Float(value) if value.is_finite() => parse_f64_to_i64(value.round_ties_even()),
        ScalarValue::Float(_) | ScalarValue::Null => None,
        ScalarValue::Str(value) | ScalarValue::Json(value) => value.parse().ok(),
    }
}

fn decode_bytea(value: &str) -> Result<(), QueryError> {
    if !value.starts_with("\\x") {
        return Err(QueryError::General(
            "cannot cast value to BYTEA".to_string(),
        ));
    }
    if (value.len() - 2).rem_euclid(2) != 0 {
        return Err(QueryError::General(
            "cannot cast value to BYTEA".to_string(),
        ));
    }
    for byte in &value.as_bytes()[2..] {
        if !byte.is_ascii_hexdigit() {
            return Err(QueryError::General(
                "cannot cast value to BYTEA".to_string(),
            ));
        }
    }
    Ok(())
}

fn binary_scalar(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    let result = match op {
        BinaryOp::And => left
            .predicate_result()?
            .and(right.predicate_result()?)
            .into_scalar(),
        BinaryOp::Or => left
            .predicate_result()?
            .or(right.predicate_result()?)
            .into_scalar(),
        BinaryOp::Eq => comparison_result(eq_value(left, right)),
        BinaryOp::NotEq => comparison_result(eq_value(left, right).map(|value| !value)),
        BinaryOp::Lt => comparison_result(ordered_cmp(left, right, std::cmp::Ordering::is_lt)),
        BinaryOp::Lte => comparison_result(ordered_cmp(left, right, |ordering| !ordering.is_gt())),
        BinaryOp::Gt => comparison_result(ordered_cmp(left, right, std::cmp::Ordering::is_gt)),
        BinaryOp::Gte => comparison_result(ordered_cmp(left, right, |ordering| !ordering.is_lt())),
        BinaryOp::Like => comparison_result(like_match(left.as_str(), right.as_str())?),
        BinaryOp::Add => checked_math_result(left, right, i64::checked_add, |a, b| a + b)?,
        BinaryOp::Sub => checked_math_result(left, right, i64::checked_sub, |a, b| a - b)?,
        BinaryOp::Mul => checked_math_result(left, right, i64::checked_mul, |a, b| a * b)?,
        BinaryOp::Div => {
            if right.to_f64().is_some_and(|value| value == 0.0) && left.to_f64().is_some() {
                return Err(QueryError::General("division by zero".to_string()));
            }
            // Integer operands divide with truncation toward zero, as in
            // PostgreSQL; `i64::MIN / -1` is the one overflowing quotient.
            checked_math_result(left, right, i64::checked_div, |a, b| a / b)?
        }
        BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
            if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
                ScalarValue::Null
            } else {
                ScalarValue::Float(vector_distance(op, left, right)?)
            }
        }
    };
    Ok(result)
}

fn comparison_result(result: Option<bool>) -> ScalarValue {
    result.map_or(ScalarValue::Null, ScalarValue::Bool)
}

fn like_match(value: Option<&str>, pattern: Option<&str>) -> Result<Option<bool>, QueryError> {
    let (Some(value), Some(pattern)) = (value, pattern) else {
        return Ok(None);
    };
    like::like_matches(value, pattern)
        .map(Some)
        .map_err(|error| QueryError::General(error.to_string()))
}

fn math_result(
    left: &ScalarValue,
    right: &ScalarValue,
    op: impl Fn(f64, f64) -> f64,
) -> ScalarValue {
    binary_math(left, right, op).map_or(ScalarValue::Null, ScalarValue::Float)
}

fn checked_math_result(
    left: &ScalarValue,
    right: &ScalarValue,
    int_op: impl Fn(i64, i64) -> Option<i64>,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<ScalarValue, QueryError> {
    if let (ScalarValue::Int(left), ScalarValue::Int(right)) = (left, right) {
        return int_op(*left, *right)
            .map(ScalarValue::Int)
            .ok_or_else(|| QueryError::General(BIGINT_OUT_OF_RANGE.to_string()));
    }
    Ok(math_result(left, right, float_op))
}

fn ordered_cmp(
    left: &ScalarValue,
    right: &ScalarValue,
    cmp: impl Fn(std::cmp::Ordering) -> bool,
) -> Option<bool> {
    if let Some(ordering) = compare_numeric_values(&left.to_value(), &right.to_value()) {
        return Some(cmp(ordering));
    }
    match (left.to_f64(), right.to_f64()) {
        (Some(left), Some(right)) => left.partial_cmp(&right).map(cmp),
        _ => left
            .as_str()
            .zip(right.as_str())
            .map(|(left, right)| cmp(compare_text(left, right))),
    }
}

/// Compares strings the way the executor orders them everywhere else:
/// canonical timestamps written in the pre-fixed-width `...SSZ` form are
/// widened first so whole and fractional seconds compare by instant.
fn compare_text(left: &str, right: &str) -> std::cmp::Ordering {
    use crate::types::temporal::timestamp_order_text;
    timestamp_order_text(left).cmp(&timestamp_order_text(right))
}

fn binary_math(
    left: &ScalarValue,
    right: &ScalarValue,
    op: impl Fn(f64, f64) -> f64,
) -> Option<f64> {
    left.to_f64()
        .zip(right.to_f64())
        .map(|(left, right)| op(left, right))
}

fn vector_distance(
    op: &BinaryOp,
    left: &ScalarValue,
    right: &ScalarValue,
) -> Result<f64, QueryError> {
    let left = left
        .as_str()
        .and_then(parse_vector_text)
        .ok_or_else(|| QueryError::General("invalid vector distance operand".to_string()))?;
    let right = right
        .as_str()
        .and_then(parse_vector_text)
        .ok_or_else(|| QueryError::General("invalid vector distance operand".to_string()))?;
    if left.is_empty() || right.is_empty() {
        return Err(QueryError::General(
            "vector distance operand cannot be empty".to_string(),
        ));
    }
    if left.len() != right.len() {
        return Err(QueryError::General(format!(
            "vector distance dimension mismatch: {} != {}",
            left.len(),
            right.len()
        )));
    }

    Ok(match op {
        BinaryOp::PgvectorCosine => crate::vector::cosine_distance(&left, &right),
        BinaryOp::PgvectorL2 => crate::vector::l2_distance(&left, &right),
        BinaryOp::PgvectorDot => -crate::vector::dot_score(&left, &right),
        _ => 0.0,
    })
}

fn eq_value(left: &ScalarValue, right: &ScalarValue) -> Option<bool> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return None;
    }

    match (left, right) {
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => Some(left == right),
        (
            ScalarValue::Int(_) | ScalarValue::Float(_),
            ScalarValue::Int(_) | ScalarValue::Float(_),
        ) => compare_numeric_values(&left.to_value(), &right.to_value())
            .map(std::cmp::Ordering::is_eq),
        (ScalarValue::Str(left), ScalarValue::Str(right)) => {
            Some(compare_text(left, right).is_eq())
        }
        (ScalarValue::Json(left), ScalarValue::Str(right) | ScalarValue::Json(right))
        | (ScalarValue::Str(left), ScalarValue::Json(right)) => json_equality::equal(left, right),
        (ScalarValue::Bool(left), ScalarValue::Int(right)) => {
            Some((*left && *right != 0) || (!*left && *right == 0))
        }
        (ScalarValue::Int(left), ScalarValue::Bool(right)) => {
            Some((*left != 0 && *right) || (*left == 0 && !*right))
        }
        (ScalarValue::Bool(left), ScalarValue::Float(right)) => {
            Some((*left && *right != 0.0) || (!*left && *right == 0.0))
        }
        (ScalarValue::Float(left), ScalarValue::Bool(right)) => {
            Some((*left != 0.0 && *right) || (*left == 0.0 && !*right))
        }
        _ => None,
    }
}

fn scalar_from_value(value: &Value) -> ScalarValue {
    match value {
        Value::Bool(v) => ScalarValue::Bool(*v),
        Value::Int64(v) => ScalarValue::Int(*v),
        Value::Float64(v) => ScalarValue::Float(*v),
        Value::String(v) => ScalarValue::Str(v.clone()),
        Value::Vector(v) => ScalarValue::Str(format!(
            "[{}]",
            v.values
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )),
        Value::Json(v) => ScalarValue::Json(v.to_string()),
        Value::Null => ScalarValue::Null,
    }
}

fn eval_column_value<R: RowAccess + ?Sized>(
    row: &R,
    name: &str,
    local_args: Option<&HashMap<String, Value>>,
) -> ScalarValue {
    column_value_ref(row, name, local_args).map_or(ScalarValue::Null, scalar_from_value)
}

// Both evaluators resolve the same canonical local binding before the row.
// Only the value-returning evaluator clones the original rich Value carrier.
fn column_value_ref<'a, R: RowAccess + ?Sized>(
    row: &'a R,
    name: &str,
    local_args: Option<&'a HashMap<String, Value>>,
) -> Option<&'a Value> {
    if let Some(local_args) = local_args {
        let key = crate::sql::ColumnIdentifierPath::parse(name)
            .map_or_else(|_| name.to_ascii_lowercase(), |column| column.lookup_key());
        if let Some(value) = local_args.get(&key) {
            return Some(value);
        }
    }
    row.get(name)
}

fn evaluate_function_scalar<R: RowAccess + ?Sized>(
    row: &R,
    function: &FunctionCall,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    evaluate_function(function, row, context).map(|value| scalar_from_value(&value))
}

fn eval_binary_expr<R: RowAccess + ?Sized>(
    row: &R,
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let left_value = eval_scalar_with_context(row, left, context)?;
    let right_value = eval_scalar_with_context(row, right, context)?;
    if matches!(
        op,
        BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    ) {
        if let Some(ordering) = super::array_order::compare(
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

fn eval_is_null_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    negated: bool,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let value = eval_scalar_with_context(row, expr, context)?;
    let is_null = matches!(value, ScalarValue::Null);
    Ok(ScalarValue::Bool(if negated { !is_null } else { is_null }))
}

fn eval_in_list_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    values: &[Expr],
    negated: bool,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let left = eval_scalar_with_context(row, expr, context)?;
    let mut result = PredicateResult::False;
    for value in values {
        let right = eval_scalar_with_context(row, value, context)?;
        result = match eq_value(&left, &right) {
            Some(true) => PredicateResult::True,
            None if matches!(result, PredicateResult::False) => PredicateResult::Unknown,
            Some(false) | None => result,
        };
        if result.is_true() {
            break;
        }
    }
    if negated {
        Ok(result.not().into_scalar())
    } else {
        Ok(result.into_scalar())
    }
}

fn eval_between_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    low: &Expr,
    high: &Expr,
    negated: bool,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let value = eval_scalar_with_context(row, expr, context)?;
    let low_value = eval_scalar_with_context(row, low, context)?;
    let high_value = eval_scalar_with_context(row, high, context)?;
    let in_range = super::array_order::compare(
        row,
        (expr, &value.to_value()),
        (low, &low_value.to_value()),
        context.user_functions,
        context.local_args,
    )
    .map(|ordering| !ordering.is_lt())
    .or_else(|| ordered_cmp(&value, &low_value, |ordering| !ordering.is_lt()))
    .map_or(PredicateResult::Unknown, bool_to_predicate)
    .and(
        super::array_order::compare(
            row,
            (expr, &value.to_value()),
            (high, &high_value.to_value()),
            context.user_functions,
            context.local_args,
        )
        .map(|ordering| !ordering.is_gt())
        .or_else(|| ordered_cmp(&value, &high_value, |ordering| !ordering.is_gt()))
        .map_or(PredicateResult::Unknown, bool_to_predicate),
    );
    if negated {
        Ok(in_range.not().into_scalar())
    } else {
        Ok(in_range.into_scalar())
    }
}

fn eval_not_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let value = eval_scalar_with_context(row, expr, context)?;
    Ok(value.predicate_result()?.not().into_scalar())
}

fn bool_to_predicate(value: bool) -> PredicateResult {
    if value {
        PredicateResult::True
    } else {
        PredicateResult::False
    }
}

fn eval_cast_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    data_type: &DataType,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    let value = eval_scalar_with_context(row, expr, context)?;
    cast_scalar(&value, data_type)
}

fn parse_f64_to_i64(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    format!("{value:.0}").parse::<i64>().ok()
}

#[cfg(test)]
#[path = "filter/tests.rs"]
mod tests;

#[path = "filter/json_equality.rs"]
mod json_equality;
