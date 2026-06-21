use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::{Batch, RowAccess};
use crate::executor::QueryError;
use crate::search::analyzer::AnalyzerConfig;
use crate::sql::ast::FunctionCall;
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{DataType, Value};
use uuid::Uuid;

#[path = "filter/functions.rs"]
mod functions;
#[path = "filter/search.rs"]
mod search;

use functions::{evaluate_function, parse_vector_text};
#[cfg(test)]
pub(crate) use search::{prepare_query_terms, SingleFieldSearchContext};
pub(crate) use search::{prepare_query_terms_with_analyzer, SearchContext, SearchTermStats};

thread_local! {
    static FUNCTION_CACHE: RefCell<FunctionCache> = RefCell::new(FunctionCache::new());
}

const FUNCTION_CACHE_SIZE: usize = 256;

struct FunctionCache {
    keys: Vec<u64>,
    values: Vec<Value>,
}

impl FunctionCache {
    fn new() -> Self {
        Self {
            keys: Vec::with_capacity(FUNCTION_CACHE_SIZE),
            values: Vec::with_capacity(FUNCTION_CACHE_SIZE),
        }
    }

    fn lookup(&self, key: u64) -> Option<&Value> {
        self.keys
            .iter()
            .position(|k| *k == key)
            .and_then(|i| self.values.get(i))
    }

    fn store(&mut self, key: u64, value: Value) {
        if let Some(pos) = self.keys.iter().position(|k| *k == key) {
            self.values[pos] = value;
            return;
        }
        if self.keys.len() >= FUNCTION_CACHE_SIZE {
            self.keys.remove(0);
            self.values.remove(0);
        }
        self.keys.push(key);
        self.values.push(value);
    }
}

fn function_cache_key(name: &str, args: &[Value]) -> u64 {
    use std::hash::Hasher;
    fn hash_value(hasher: &mut std::hash::DefaultHasher, value: &Value) {
        match value {
            Value::Null => 0u8.hash(hasher),
            Value::Bool(v) => {
                1u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Int64(v) => {
                2u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Float64(v) => {
                3u8.hash(hasher);
                v.to_bits().hash(hasher);
            }
            Value::String(v) => {
                4u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Vector(v) => {
                5u8.hash(hasher);
                v.values.len().hash(hasher);
            }
            Value::Json(v) => {
                6u8.hash(hasher);
                v.to_string().hash(hasher);
            }
        }
    }
    let mut hasher = std::hash::DefaultHasher::new();
    name.hash(&mut hasher);
    for arg in args {
        hash_value(&mut hasher, arg);
    }
    hasher.finish()
}

fn has_only_constant_args(exprs: &[Expr]) -> bool {
    exprs.iter().all(is_constant_expr)
}

fn is_constant_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::Param(_) => true,
        Expr::Function(f) => f.args.iter().all(is_constant_expr),
        Expr::Binary { left, right, .. } => is_constant_expr(left) && is_constant_expr(right),
        Expr::Cast { expr, .. } => is_constant_expr(expr),
        Expr::InList { expr, values, .. } => {
            is_constant_expr(expr) && values.iter().all(is_constant_expr)
        }
        Expr::Not { expr } => is_constant_expr(expr),
        Expr::IsNull { expr, .. } => is_constant_expr(expr),
        Expr::Between {
            expr, low, high, ..
        } => is_constant_expr(expr) && is_constant_expr(low) && is_constant_expr(high),
        _ => false,
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ScalarValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl ScalarValue {
    pub(crate) fn as_bool(&self) -> bool {
        match self {
            ScalarValue::Bool(v) => *v,
            ScalarValue::Int(v) => *v != 0,
            ScalarValue::Float(v) => *v != 0.0,
            ScalarValue::Str(v) => !v.is_empty(),
            ScalarValue::Null => false,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            ScalarValue::Str(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn to_f64(&self) -> Option<f64> {
        match self {
            ScalarValue::Float(v) => Some(*v),
            ScalarValue::Int(v) => Some(*v as f64),
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
            ScalarValue::Str(v) => Value::String(v.clone()),
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
    Ok(rows
        .into_iter()
        .filter(|row| {
            eval_filter(
                row,
                expression,
                params,
                search_context,
                user_functions,
                session,
            )
            .unwrap_or(false)
        })
        .collect())
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
    match expr {
        Expr::Function(function) => evaluate_function(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        ),
        _ => Ok(eval_scalar(
            row,
            expr,
            params,
            search_context,
            user_functions,
            local_args,
            session,
        )?
        .to_value()),
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
    let value = eval_scalar(
        row,
        expression,
        params,
        search_context,
        user_functions,
        None,
        session,
    )?;
    Ok(value.as_bool())
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
    match expr {
        Expr::Column(name) => {
            if let Some(local_args) = local_args {
                let key = name.to_ascii_lowercase();
                if let Some(value) = local_args.get(&key) {
                    return Ok(scalar_from_value(value).unwrap_or(ScalarValue::Null));
                }
            }

            Ok(row
                .get(name)
                .and_then(scalar_from_value)
                .unwrap_or(ScalarValue::Null))
        }
        Expr::StringLiteral(value) => Ok(ScalarValue::Str(value.clone())),
        Expr::NumberLiteral(value) => Ok(ScalarValue::Float(*value)),
        Expr::BoolLiteral(value) => Ok(ScalarValue::Bool(*value)),
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Param(index) => params
            .get(*index)
            .and_then(scalar_from_value)
            .map(Ok)
            .unwrap_or_else(|| Ok(ScalarValue::Null)),
        Expr::Function(function) => Ok(scalar_from_value(&evaluate_function(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
            local_args,
        )?)
        .unwrap_or(ScalarValue::Null)),
        Expr::Binary { left, op, right } => Ok(binary_scalar(
            &eval_scalar(
                row,
                left,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?,
            op,
            &eval_scalar(
                row,
                right,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?,
        )),
        Expr::IsNull { expr, negated } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let is_null = matches!(value, ScalarValue::Null);
            Ok(ScalarValue::Bool(if *negated { !is_null } else { is_null }))
        }
        Expr::InList {
            expr,
            values,
            negated,
        } => {
            let left = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let contains = values
                .iter()
                .map(|value| {
                    eval_scalar(
                        row,
                        value,
                        params,
                        search_context,
                        user_functions,
                        local_args,
                        session,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|right| eq_value(&left, right));
            Ok(ScalarValue::Bool(if *negated {
                !contains
            } else {
                contains
            }))
        }
        Expr::Between {
            expr,
            low,
            high,
            negated,
        } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let low = eval_scalar(
                row,
                low,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let high = eval_scalar(
                row,
                high,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            let in_range = number_cmp(&value, &low, |left, right| left >= right)
                && number_cmp(&value, &high, |left, right| left <= right);
            Ok(ScalarValue::Bool(if *negated {
                !in_range
            } else {
                in_range
            }))
        }
        Expr::Not { expr } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            Ok(ScalarValue::Bool(!value.as_bool()))
        }
        Expr::Cast { expr, data_type } => {
            let value = eval_scalar(
                row,
                expr,
                params,
                search_context,
                user_functions,
                local_args,
                session,
            )?;
            cast_scalar(&value, data_type)
        }
        Expr::Exists(_) => Err(QueryError::General(
            "EXISTS predicate was not resolved before filtering".to_string(),
        )),
    }
}

fn cast_scalar(value: &ScalarValue, data_type: &DataType) -> Result<ScalarValue, QueryError> {
    if matches!(value, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }

    match data_type {
        DataType::Null => Ok(ScalarValue::Null),
        DataType::SmallInt => scalar_to_i64(value)
            .and_then(|value| i16::try_from(value).ok())
            .map(|value| ScalarValue::Int(value as i64))
            .ok_or_else(|| QueryError::General("cannot cast value to SMALLINT".to_string())),
        DataType::Int => scalar_to_i64(value)
            .and_then(|value| i32::try_from(value).ok())
            .map(|value| ScalarValue::Int(value as i64))
            .ok_or_else(|| QueryError::General("cannot cast value to INT".to_string())),
        DataType::BigInt => scalar_to_i64(value)
            .map(ScalarValue::Int)
            .ok_or_else(|| QueryError::General("cannot cast value to BIGINT".to_string())),
        DataType::Float => value
            .to_f64()
            .map(ScalarValue::Float)
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(ScalarValue::Float)
            })
            .ok_or_else(|| QueryError::General("cannot cast value to FLOAT".to_string())),
        DataType::Boolean => match value {
            ScalarValue::Bool(value) => Ok(ScalarValue::Bool(*value)),
            ScalarValue::Int(value) => Ok(ScalarValue::Bool(*value != 0)),
            ScalarValue::Float(value) => Ok(ScalarValue::Bool(*value != 0.0)),
            ScalarValue::Str(value) => match value.to_ascii_lowercase().as_str() {
                "true" | "t" | "1" => Ok(ScalarValue::Bool(true)),
                "false" | "f" | "0" => Ok(ScalarValue::Bool(false)),
                _ => Err(QueryError::General(
                    "cannot cast value to BOOLEAN".to_string(),
                )),
            },
            ScalarValue::Null => Ok(ScalarValue::Null),
        },
        DataType::Text => Ok(ScalarValue::Str(match value {
            ScalarValue::Bool(value) => value.to_string(),
            ScalarValue::Int(value) => value.to_string(),
            ScalarValue::Float(value) => value.to_string(),
            ScalarValue::Str(value) => value.clone(),
            ScalarValue::Null => String::new(),
        })),
        DataType::Char { length } => {
            let value = cast_to_text(value)
                .filter(|value| value.chars().count() <= length.unwrap_or(1) as usize)
                .ok_or_else(|| QueryError::General("cannot cast value to CHAR".to_string()))?;
            Ok(ScalarValue::Str(value))
        }
        DataType::Varchar { length } => {
            let value = cast_to_text(value)
                .filter(|value| {
                    length.is_none_or(|length| value.chars().count() <= length as usize)
                })
                .ok_or_else(|| QueryError::General("cannot cast value to VARCHAR".to_string()))?;
            Ok(ScalarValue::Str(value))
        }
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to BYTEA".to_string()))?;
            decode_bytea(value)?;
            Ok(ScalarValue::Str(value.to_string()))
        }
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to UUID".to_string()))?;
            Uuid::parse_str(value)
                .map_err(|_| QueryError::General("cannot cast value to UUID".to_string()))?;
            Ok(ScalarValue::Str(value.to_string()))
        }
        DataType::Date | DataType::Time | DataType::Timestamp => match value {
            ScalarValue::Str(value) => Ok(ScalarValue::Str(value.clone())),
            _ => Err(QueryError::General(
                "cannot cast value to timestamp/time/date type".to_string(),
            )),
        },
        DataType::Json => Ok(ScalarValue::Str(match value {
            ScalarValue::Bool(value) => value.to_string(),
            ScalarValue::Int(value) => value.to_string(),
            ScalarValue::Float(value) => value.to_string(),
            ScalarValue::Str(value) => value.clone(),
            ScalarValue::Null => "null".to_string(),
        })),
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
        ScalarValue::Str(value) => Some(value.clone()),
        ScalarValue::Null => None,
    }
}

fn scalar_to_i64(value: &ScalarValue) -> Option<i64> {
    match value {
        ScalarValue::Int(value) => Some(*value),
        ScalarValue::Bool(value) => Some(if *value { 1 } else { 0 }),
        ScalarValue::Float(value) if value.is_finite() && value.fract() == 0.0 => {
            Some(*value as i64)
        }
        ScalarValue::Float(_) => None,
        ScalarValue::Str(value) => value.parse().ok(),
        ScalarValue::Null => None,
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
    for byte in value.as_bytes()[2..].iter() {
        if !byte.is_ascii_hexdigit() {
            return Err(QueryError::General(
                "cannot cast value to BYTEA".to_string(),
            ));
        }
    }
    Ok(())
}

fn binary_scalar(left: &ScalarValue, op: &BinaryOp, right: &ScalarValue) -> ScalarValue {
    let op = op.clone();
    match op {
        BinaryOp::And => ScalarValue::Bool(left.as_bool() && right.as_bool()),
        BinaryOp::Or => ScalarValue::Bool(left.as_bool() || right.as_bool()),
        BinaryOp::Eq => ScalarValue::Bool(eq_value(left, right)),
        BinaryOp::NotEq => ScalarValue::Bool(!eq_value(left, right)),
        BinaryOp::Lt => ScalarValue::Bool(number_cmp(left, right, |l, r| l < r)),
        BinaryOp::Lte => ScalarValue::Bool(number_cmp(left, right, |l, r| l <= r)),
        BinaryOp::Gt => ScalarValue::Bool(number_cmp(left, right, |l, r| l > r)),
        BinaryOp::Gte => ScalarValue::Bool(number_cmp(left, right, |l, r| l >= r)),
        BinaryOp::Like => ScalarValue::Bool(like_match(left.as_str(), right.as_str())),
        BinaryOp::Add => ScalarValue::Float(binary_math(left, right, |a, b| a + b)),
        BinaryOp::Sub => ScalarValue::Float(binary_math(left, right, |a, b| a - b)),
        BinaryOp::Mul => ScalarValue::Float(binary_math(left, right, |a, b| a * b)),
        BinaryOp::Div => {
            ScalarValue::Float(binary_math(
                left,
                right,
                |a, b| {
                    if b == 0.0 {
                        0.0
                    } else {
                        a / b
                    }
                },
            ))
        }
        BinaryOp::PgvectorCosine => ScalarValue::Float(vector_distance(op, left, right)),
        BinaryOp::PgvectorL2 => ScalarValue::Float(vector_distance(op, left, right)),
        BinaryOp::PgvectorDot => ScalarValue::Float(vector_distance(op, left, right)),
    }
}

fn like_match(value: Option<&str>, pattern: Option<&str>) -> bool {
    match (value, pattern) {
        (Some(value), Some(pattern)) => {
            let value = value.to_lowercase();
            let pattern = pattern.to_lowercase();
            if pattern == "%" {
                return true;
            }
            if !pattern.starts_with('%') && !pattern.ends_with('%') {
                return value == pattern;
            }
            if pattern.starts_with('%') && pattern.ends_with('%') {
                let contains_expr = pattern.trim_matches('%');
                return value.contains(contains_expr);
            }
            if pattern.starts_with('%') {
                let suffix = pattern.trim_start_matches('%');
                return value.ends_with(suffix);
            }
            if pattern.ends_with('%') {
                let prefix = pattern.trim_end_matches('%');
                return value.starts_with(prefix);
            }
            false
        }
        _ => false,
    }
}

fn number_cmp(left: &ScalarValue, right: &ScalarValue, cmp: impl Fn(f64, f64) -> bool) -> bool {
    cmp(left.to_f64().unwrap_or(0.0), right.to_f64().unwrap_or(0.0))
}

fn binary_math(left: &ScalarValue, right: &ScalarValue, op: impl Fn(f64, f64) -> f64) -> f64 {
    op(left.to_f64().unwrap_or(0.0), right.to_f64().unwrap_or(0.0))
}

fn vector_distance(op: BinaryOp, left: &ScalarValue, right: &ScalarValue) -> f64 {
    let left = left
        .as_str()
        .and_then(parse_vector_text)
        .unwrap_or_default();
    let right = right
        .as_str()
        .and_then(parse_vector_text)
        .unwrap_or_default();
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return f64::INFINITY;
    }

    match op {
        BinaryOp::PgvectorCosine => crate::vector::cosine_distance(&left, &right),
        BinaryOp::PgvectorL2 => crate::vector::l2_distance(&left, &right),
        BinaryOp::PgvectorDot => -crate::vector::dot_score(&left, &right),
        _ => 0.0,
    }
}

fn eq_value(left: &ScalarValue, right: &ScalarValue) -> bool {
    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => true,
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => left == right,
        (ScalarValue::Int(left), ScalarValue::Int(right)) => left == right,
        (ScalarValue::Float(left), ScalarValue::Float(right)) => left == right,
        (ScalarValue::Str(left), ScalarValue::Str(right)) => left == right,
        (ScalarValue::Int(left), ScalarValue::Float(right)) => (*left as f64) == *right,
        (ScalarValue::Float(left), ScalarValue::Int(right)) => *left == (*right as f64),
        (ScalarValue::Bool(left), ScalarValue::Int(right)) => {
            (*left && *right != 0) || (!*left && *right == 0)
        }
        (ScalarValue::Int(left), ScalarValue::Bool(right)) => {
            (*left != 0 && *right) || (*left == 0 && !*right)
        }
        (ScalarValue::Bool(left), ScalarValue::Float(right)) => {
            (*left && *right != 0.0) || (!*left && *right == 0.0)
        }
        (ScalarValue::Float(left), ScalarValue::Bool(right)) => {
            (*left != 0.0 && *right) || (*left == 0.0 && !*right)
        }
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        _ => false,
    }
}

fn scalar_from_value(value: &Value) -> Option<ScalarValue> {
    match value {
        Value::Bool(v) => Some(ScalarValue::Bool(*v)),
        Value::Int64(v) => Some(ScalarValue::Int(*v)),
        Value::Float64(v) => Some(ScalarValue::Float(*v)),
        Value::String(v) => Some(ScalarValue::Str(v.clone())),
        Value::Vector(v) => Some(ScalarValue::Str(format!(
            "[{}]",
            v.values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ))),
        Value::Json(v) => Some(ScalarValue::Str(v.to_string())),
        Value::Null => Some(ScalarValue::Null),
    }
}

#[cfg(test)]
#[path = "filter/tests.rs"]
mod tests;
