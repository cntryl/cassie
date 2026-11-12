#[path = "filter/binary_expr.rs"]
mod binary_expr;
use binary_expr::eval_binary_expr;
#[path = "filter/cast_admission.rs"]
mod cast_admission;
#[path = "filter/cast_expr.rs"]
mod cast_expr;
use cast_expr::eval_cast_expr;

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

/// PostgreSQL's message (SQLSTATE 22003) for an overflowing int8 result.
pub(crate) const BIGINT_OUT_OF_RANGE: &str = "bigint out of range";

#[path = "filter/exists_value.rs"]
mod exists_value;
pub(crate) use exists_value::{evaluate_resolving_exists, ExistsValueContext};
/// Resolves a reached `EXISTS` or sublink node to its executed form.
pub(crate) type ExistsResolver<'a> = dyn Fn(&Expr) -> Result<Expr, QueryError> + 'a;
#[path = "filter/json_concat.rs"]
mod json_concat;
#[path = "filter/json_containment.rs"]
mod json_containment;
#[path = "filter/json_delete.rs"]
mod json_delete;
#[cfg(test)]
pub(crate) use json_delete::cancel_function_path_scan_once;
#[cfg(test)]
pub(crate) use json_delete::cancel_raw_scan_once;
#[path = "filter/json_document.rs"]
mod json_document;
#[path = "filter/json_extract.rs"]
mod json_extract;
#[cfg(test)]
#[path = "filter/json_extract_shared_owner_tests.rs"]
mod json_extract_shared_owner_tests;
#[path = "filter/json_key_exists.rs"]
mod json_key_exists;
#[path = "filter/json_order.rs"]
mod json_order;
#[path = "filter/json_path_operator.rs"]
mod json_path_operator;
#[path = "filter/json_scalar_leaf.rs"]
mod json_scalar_leaf;
#[cfg(test)]
pub(crate) use json_scalar_leaf::cancel_copy_once as cancel_json_leaf_copy_once;
#[cfg(test)]
pub(crate) use json_scalar_leaf::clear_copy_cancellation as clear_json_leaf_copy_cancellation;
#[path = "filter/quantified_array.rs"]
mod quantified_array;
#[path = "filter/subquery_value.rs"]
mod subquery_value;

#[path = "filter/binary.rs"]
mod binary;
#[path = "filter/vector_distance.rs"]
mod vector_distance;
use vector_distance::vector_distance;
#[path = "filter/array_identity.rs"]
mod array_identity;
#[path = "filter/array_operators.rs"]
mod array_operators;

pub(crate) fn compare_sql_arrays(
    left: &crate::types::SqlArray,
    right: &crate::types::SqlArray,
) -> Option<std::cmp::Ordering> {
    array_operators::compare_arrays(left, right)
}
pub(crate) fn compare_sql_arrays_controlled(
    left: &crate::types::SqlArray,
    right: &crate::types::SqlArray,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<Option<std::cmp::Ordering>, crate::app::CassieError> {
    array_operators::compare_arrays_controlled(left, right, controls)
}
#[path = "filter/array_subscript.rs"]
mod array_subscript;
#[path = "filter/bytea_casts.rs"]
mod bytea_casts;
#[path = "filter/coalesce.rs"]
mod coalesce;
#[path = "filter/conditional.rs"]
mod conditional;
#[path = "filter/extended_ops.rs"]
pub(crate) mod extended_ops;
#[path = "filter/hash_functions.rs"]
pub(crate) mod hash_functions;
use binary::binary_scalar;
#[path = "filter/numeric_comparison.rs"]
mod numeric_comparison;
#[path = "filter/numeric_ops.rs"]
mod numeric_ops;
#[path = "filter/object_ids.rs"]
mod object_ids;
#[path = "filter/sequence_functions.rs"]
mod sequence_functions;
#[path = "filter/comment_functions.rs"]
mod comment_functions;
pub(crate) use casts::cast_scratch_bytes;
pub(crate) use numeric_comparison::comparison_key_value;
#[path = "filter/functions.rs"]
mod functions;
#[path = "filter/like.rs"]
mod like;
#[path = "filter/number_format.rs"]
mod number_format;
#[path = "filter/search.rs"]
mod search;
#[path = "filter/temporal.rs"]
pub(crate) mod temporal;
#[path = "filter/temporal_format.rs"]
mod temporal_format;
#[path = "filter/temporal_functions.rs"]
mod temporal_functions;
#[path = "filter/unary_numeric.rs"]
mod unary_numeric;

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
    exists: Option<&'a ExistsResolver<'a>>,
    controls: Option<&'a crate::runtime::QueryExecutionControls>,
    memory: Option<&'a evaluation::EvaluationMemory>,
    deletion_memory: Option<&'a evaluation::EvaluationMemory>,
}

#[cfg(test)]
#[path = "filter/json_delete_evaluation_tests.rs"]
mod json_delete_evaluation_tests;

#[cfg(test)]
#[path = "filter/json_function_result_owner_tests.rs"]
mod json_function_result_owner_tests;

#[cfg(test)]
thread_local! {
    static JSON_FUNCTION_CALL_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ARRAY_VALUE_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_json_function_call_count() {
    JSON_FUNCTION_CALL_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn take_json_function_call_count() -> usize {
    JSON_FUNCTION_CALL_COUNT.with(|count| count.replace(0))
}

#[cfg(test)]
fn take_array_value_clone_count() -> usize {
    ARRAY_VALUE_CLONE_COUNT.with(|count| count.replace(0))
}

#[derive(Debug, Clone)]
pub(crate) enum ScalarValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Json(String),
    Typed(crate::types::typed::TypedValue),
    Array(crate::types::SqlArray),
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
            | ScalarValue::Json(_)
            | ScalarValue::Typed(_)
            | ScalarValue::Array(_) => Err(QueryError::General(
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
            ScalarValue::Typed(v) => v.as_f64(),
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
            ScalarValue::Typed(v) => Value::Typed(v.clone()),
            ScalarValue::Array(v) => {
                #[cfg(test)]
                ARRAY_VALUE_CLONE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
                Value::Array(v.clone())
            }
        }
    }

    fn into_value(self) -> Value {
        match self {
            ScalarValue::Null => Value::Null,
            ScalarValue::Bool(value) => Value::Bool(value),
            ScalarValue::Int(value) => Value::Int64(value),
            ScalarValue::Float(value) => Value::Float64(value),
            ScalarValue::Str(value) | ScalarValue::Json(value) => Value::String(value),
            ScalarValue::Typed(value) => Value::Typed(value),
            ScalarValue::Array(value) => Value::Array(value),
        }
    }
}

/// Casts a value to `target` with the evaluator's `CAST` semantics.
pub(crate) fn cast_value(value: &Value, target: &DataType) -> Result<Value, QueryError> {
    cast_scalar(&scalar_from_value(value), target, None).map(|value| value.to_value())
}

#[path = "filter/filtering.rs"]
mod filtering;
pub(crate) use filtering::{filter_batches, filter_batches_with_controls, filter_rows};

#[path = "filter/evaluation.rs"]
pub(crate) mod evaluation;
pub(crate) use evaluation::evaluate_source_argument_resolving;
pub(crate) use evaluation::{evaluate_expr_value, evaluate_source_argument, EvaluationMemory};

pub(super) fn evaluate_expr_value_with_context<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    if let Some(child) = array_identity::identity_cast_child(expr) {
        return evaluate_expr_value_with_context(row, child, context);
    }
    let _statement_scratch = context
        .session
        .map(|session| session.statement_text_scratch(expr))
        .transpose()?;
    match expr {
        Expr::Param(index) => {
            evaluation::admit_leaf(row, expr, context)?;
            let value = context.params.get(*index);
            #[cfg(test)]
            functions::record_input_value_clone(value.is_some());
            Ok(value.cloned().unwrap_or(Value::Null))
        }
        Expr::Column(name) => {
            evaluation::admit_leaf(row, expr, context)?;
            let value = column_value_ref(row, name, context.local_args);
            #[cfg(test)]
            functions::record_input_value_clone(value.is_some());
            Ok(value.cloned().unwrap_or(Value::Null))
        }
        Expr::Function(function) => evaluate_function(function, row, context),
        Expr::Subquery(subquery) => subquery_value::value(row, expr, subquery, context),
        _ => Ok(eval_scalar_with_context(row, expr, context)?.into_value()),
    }
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
            exists: None,
            controls: None,
            memory: None,
            deletion_memory: None,
        },
    )
}

#[path = "filter/casts.rs"]
mod casts;
#[path = "filter/character_arrays.rs"]
pub(crate) mod character_arrays;
#[path = "filter/constants.rs"]
mod constants;
mod like_predicate;
pub(crate) mod symmetric_between;
#[cfg(test)]
mod symmetric_between_tests;
#[path = "filter/text_arrays.rs"]
mod text_arrays;
pub(crate) use constants::constant_scalar_with_parameters;
#[path = "filter/case.rs"]
mod case;
#[path = "filter/row_predicates.rs"]
mod row_predicates;
#[path = "filter/truth_tests.rs"]
mod truth_tests;
pub(crate) use row_predicates::field_result as row_field_result;

fn eval_scalar_with_context<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    evaluation::admit_leaf(row, expr, context)?;
    let _statement_scratch = context
        .session
        .map(|session| session.statement_text_scratch(expr))
        .transpose()?;
    match expr {
        Expr::RowCompare(comparison) => row_predicates::evaluate(row, comparison, context),
        Expr::Column(name) => match column_value_ref(row, name, context.local_args) {
            Some(value) => json_scalar_leaf::from_value(value, context),
            None => Ok(ScalarValue::Null),
        },
        Expr::StringLiteral(value) => Ok(ScalarValue::Str(value.clone())),
        Expr::NumberLiteral(value) => Ok(ScalarValue::Float(*value)),
        Expr::IntegerLiteral(value) => Ok(ScalarValue::Int(*value)),
        Expr::NumericLiteral(text) => crate::types::extended::numeric::parse_text(text)
            .map(|value| ScalarValue::Typed(crate::types::typed::TypedValue::Numeric(value)))
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error))),
        Expr::BoolLiteral(value) => Ok(ScalarValue::Bool(*value)),
        Expr::Null => Ok(ScalarValue::Null),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => case::evaluate(
            row,
            operand.as_deref(),
            branches,
            else_expr.as_deref(),
            context,
        ),
        Expr::Param(index) => match context.params.get(*index) {
            Some(value) => json_scalar_leaf::from_value(value, context),
            None => Ok(ScalarValue::Null),
        },
        Expr::Function(function) => evaluate_function_scalar(row, function, context),
        Expr::ArraySubscript { array, subscripts } => {
            array_subscript::evaluate(row, array, subscripts, context)
        }
        Expr::Binary { left, op, right } => eval_binary_expr(row, left, op, right, context),
        Expr::TruthTest {
            expr,
            truth,
            negated,
        } => truth_tests::evaluate(
            &eval_scalar_with_context(row, expr, context)?,
            *truth,
            *negated,
        ),
        Expr::IsNull { expr, negated } => eval_is_null_expr(row, expr, *negated, context),
        Expr::InList {
            expr,
            values,
            negated,
        } => eval_in_list_expr(row, expr, values, *negated, context),
        expression @ Expr::Between {
            symmetric: true, ..
        } => symmetric_between::runtime(row, expression, context),
        expression @ Expr::Like { .. } => like_predicate::runtime(row, expression, context),
        Expr::Between {
            expr,
            low,
            high,
            negated,
            symmetric: false,
        } => eval_between_expr(row, expr, low, high, *negated, context),
        Expr::Not { expr } => eval_not_expr(row, expr, context),
        Expr::UnaryNumeric { expr, op } => {
            let value = eval_scalar_with_context(row, expr, context)?;
            temporal::negate(&value).unwrap_or_else(|| unary_numeric::evaluate(&value, *op))
        }
        Expr::IntegerNegate { expr, width } => {
            integers::negate(&eval_scalar_with_context(row, expr, context)?, *width)
        }
        Expr::Cast {
            expr,
            data_type,
            source_type,
            identity,
        } => eval_cast_expr(
            row,
            expr,
            data_type,
            source_type.as_ref(),
            *identity,
            context,
        ),
        Expr::Collate { expr, .. } => eval_scalar_with_context(row, expr, context),
        Expr::Exists(_) => match context.exists {
            None => Err(QueryError::General(
                "EXISTS predicate was not resolved before filtering".to_string(),
            )),
            Some(resolve) => match resolve(expr)? {
                Expr::BoolLiteral(value) => Ok(ScalarValue::Bool(value)),
                _ => Err(QueryError::General(
                    "EXISTS resolver did not return BOOLEAN".into(),
                )),
            },
        },
        Expr::Subquery(subquery) => subquery_value::evaluate(row, expr, subquery, context),
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

fn cast_vector_scalar(
    value: &ScalarValue,
    expected_dimensions: Option<usize>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    let text = value
        .as_str()
        .ok_or_else(|| QueryError::General("cannot cast value to VECTOR".to_string()))?;
    crate::vector::text::visit_components_with_dimension(text, expected_dimensions, |_| {})
        .map_err(|error| {
            let message = match error {
                crate::vector::text::TextVectorError::Shape => {
                    "VECTOR input has invalid shape or dimension".to_string()
                }
                crate::vector::text::TextVectorError::Range => {
                    "VECTOR component is outside finite f32 range".to_string()
                }
            };
            QueryError::Cassie(crate::app::CassieError::InvalidVector(message))
        })?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(ScalarValue::Str(text.to_string()))
}

fn cast_scalar(
    value: &ScalarValue,
    data_type: &DataType,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    if matches!(value, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    if let Some(result) = temporal::cast(value, data_type, controls) {
        return result;
    }

    match data_type {
        DataType::Null => Ok(ScalarValue::Null),
        DataType::SmallInt => integers::cast(value, crate::types::IntegerWidth::I16, controls),
        DataType::Int => integers::cast(value, crate::types::IntegerWidth::I32, controls),
        DataType::BigInt => integers::cast(value, crate::types::IntegerWidth::I64, controls),
        DataType::ObjectId(_) => object_ids::cast(value),
        DataType::JsonPath => {
            let path = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to JSONPATH".into()))?;
            sql_json_path::JsonPath::new(path)
                .map(|parsed| {
                    let has_variables = path.as_bytes().windows(2).any(|pair| {
                        pair[0] == b'$' && (pair[1].is_ascii_alphabetic() || pair[1] == b'_')
                    });
                    ScalarValue::Str(if has_variables {
                        path.to_owned()
                    } else {
                        parsed.to_string()
                    })
                })
                .map_err(|error| {
                    QueryError::Cassie(crate::app::CassieError::Parse(error.to_string()))
                })
        }
        DataType::Extended(kind) => kind
            .cast_from(&value.to_value())
            .map(ScalarValue::Typed)
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error))),
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
        DataType::Name => Ok(ScalarValue::Str(crate::types::name::parse_text(
            &cast_string_scalar(value),
        ))),
        DataType::Text => Ok(ScalarValue::Str(cast_string_scalar(value))),
        DataType::Char { length } => casts::character(value, *length, controls),
        DataType::Varchar { length } => casts::bounded_varchar(value, *length),
        DataType::Bytea => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to BYTEA".to_string()))?;
            crate::types::bytea_input::canonical(value)
                .map(ScalarValue::Str)
                .map_err(|error| QueryError::Cassie(error.into()))
        }
        DataType::Uuid => {
            let value = value
                .as_str()
                .ok_or_else(|| QueryError::General("cannot cast value to UUID".to_string()))?;
            let canonical = crate::types::uuid_text::parse(value)
                .ok_or_else(|| QueryError::General("cannot cast value to UUID".to_string()))?;
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
        DataType::Json => cast_json_text(value, controls).map(ScalarValue::Json),
        DataType::Jsonb => {
            let text = cast_json_text(value, controls)?;
            crate::types::jsonb::canonical_with_controls(&text, controls)
                .map(ScalarValue::Json)
                .map_err(Into::into)
        }
        DataType::Array(element) => {
            character_arrays::cast(value, data_type, None, controls, None, false)
                .unwrap_or_else(|| text_arrays::cast(value, element))
        }
        DataType::Vector(expected_dimensions) => {
            cast_vector_scalar(value, *expected_dimensions, controls)
        }
    }
}

fn cast_string_scalar(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Bool(value) => value.to_string(),
        ScalarValue::Int(value) => value.to_string(),
        ScalarValue::Float(value) => value.to_string(),
        ScalarValue::Str(value) | ScalarValue::Json(value) => value.clone(),
        ScalarValue::Array(value) => value.to_nested_json().to_string(),
        ScalarValue::Typed(value) => value.text(),
        ScalarValue::Null => String::new(),
    }
}

fn cast_to_text(value: &ScalarValue) -> Option<String> {
    match value {
        ScalarValue::Bool(value) => Some(value.to_string()),
        ScalarValue::Int(value) => Some(value.to_string()),
        ScalarValue::Float(value) => Some(value.to_string()),
        ScalarValue::Str(value) | ScalarValue::Json(value) => Some(value.clone()),
        ScalarValue::Array(value) => Some(value.to_nested_json().to_string()),
        ScalarValue::Typed(value) => Some(value.text()),
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
        ScalarValue::Typed(_) | ScalarValue::Array(_) => Err(QueryError::General(
            "cannot cast value to BOOLEAN".to_string(),
        )),
        ScalarValue::Null => Ok(ScalarValue::Null),
    }
}

fn cast_json_text(
    value: &ScalarValue,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<String, QueryError> {
    let text = match value {
        ScalarValue::Bool(value) => value.to_string(),
        ScalarValue::Int(value) => value.to_string(),
        ScalarValue::Float(value) => value.to_string(),
        ScalarValue::Str(value) | ScalarValue::Json(value) => value.clone(),
        ScalarValue::Array(value) => value.to_nested_json().to_string(),
        ScalarValue::Typed(value) => value.text(),
        ScalarValue::Null => "null".to_string(),
    };
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    serde_json::from_str::<serde::de::IgnoredAny>(&text)
        .map_err(|_| QueryError::General("invalid input syntax for type json".to_string()))?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(text)
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
    if let Some(ordering) = temporal::compare_scalars(left, right) {
        return Some(cmp(ordering));
    }
    if let Some(ordering) = extended_ops::compare(left, right) {
        return Some(cmp(ordering));
    }
    if let Some(ordering) = numeric_ops::compare(left, right) {
        return Some(cmp(ordering));
    }
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

fn eq_value(left: &ScalarValue, right: &ScalarValue) -> Option<bool> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return None;
    }
    if let Some(ordering) = temporal::compare_scalars(left, right) {
        return Some(ordering.is_eq());
    }
    if let Some(ordering) = numeric_ops::compare(left, right) {
        return Some(ordering.is_eq());
    }

    match (left, right) {
        (ScalarValue::Typed(typed), _) | (_, ScalarValue::Typed(typed))
            if crate::types::extended::operations::handles_value(typed) =>
        {
            extended_ops::compare(left, right).map(std::cmp::Ordering::is_eq)
        }
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

pub(super) fn scalar_from_value(value: &Value) -> ScalarValue {
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
        Value::Typed(v) => ScalarValue::Typed(v.clone()),
        Value::Array(v) => ScalarValue::Array(v.clone()),
        Value::Null => ScalarValue::Null,
    }
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
    #[cfg(test)]
    JSON_FUNCTION_CALL_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    let value = evaluate_function(function, row, context)?;
    let json_return = context
        .user_functions
        .get(&function.name)
        .is_some_and(|metadata| metadata.return_type.is_json())
        || crate::sql::functions::function(&function.name).is_some_and(|metadata| {
            matches!(
                metadata.return_type,
                crate::sql::functions::FunctionReturnType::Json
                    | crate::sql::functions::FunctionReturnType::Jsonb
            )
        });
    if json_return && !matches!(value, Value::Null) {
        return function_json_result(value, context);
    }
    Ok(scalar_from_value(&value))
}

fn function_json_result(value: Value, context: EvalContext<'_>) -> Result<ScalarValue, QueryError> {
    match value {
        Value::String(document) => Ok(ScalarValue::Json(document)),
        value => json_concat::into_json_document(json_scalar_leaf::from_value(&value, context)?),
    }
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
    let in_range =
        binary_expr::compare_array_operands(row, (expr, &value), (low, &low_value), context)?
            .map(|ordering| !ordering.is_lt())
            .or_else(|| ordered_cmp(&value, &low_value, |ordering| !ordering.is_lt()))
            .map_or(PredicateResult::Unknown, bool_to_predicate)
            .and(
                binary_expr::compare_array_operands(
                    row,
                    (expr, &value),
                    (high, &high_value),
                    context,
                )?
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

#[cfg(test)]
#[path = "filter/tests.rs"]
mod tests;

#[path = "filter/json_equality.rs"]
mod json_equality;

#[cfg(test)]
#[path = "filter/null_safe_tests.rs"]
mod null_safe_tests;

#[path = "filter/integers.rs"]
mod integers;

#[cfg(test)]
#[path = "filter/bytea_function_tests.rs"]
mod bytea_function_tests;
