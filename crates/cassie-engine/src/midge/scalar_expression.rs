//! Engine evaluation of the expressions stored by scalar indexes.

use std::collections::HashMap;

use cassie_storage::row_blob::RowSchema;
use cassie_storage::{ScalarExpressionEvaluationError, ScalarExpressionEvaluator, StorageError};

use crate::app::CassieError;
use crate::catalog::IndexMeta;
use crate::executor::filter;
use crate::midge::Midge;
use crate::sql::ast::Expr;
use crate::types::Value;

struct EngineScalarExpressionEvaluator<'a> {
    index_name: &'a str,
}

impl ScalarExpressionEvaluator for EngineScalarExpressionEvaluator<'_> {
    type Error = CassieError;

    fn evaluate(
        &self,
        stored_expression: &str,
        row: &[(String, Value)],
    ) -> Result<Value, CassieError> {
        evaluate_expression(self.index_name, stored_expression, row)
    }
}

pub(crate) fn key_values(
    index: &IndexMeta,
    payload: &serde_json::Value,
    row_schema: &RowSchema,
) -> Result<Option<Vec<serde_json::Value>>, CassieError> {
    let evaluator = EngineScalarExpressionEvaluator { index_name: &index.name };
    Midge::scalar_index_key_values(index, payload, row_schema, &evaluator).map_err(|error| {
        match error {
            ScalarExpressionEvaluationError::Evaluation(error) => error,
            ScalarExpressionEvaluationError::UnsupportedValue(message) => {
                CassieError::Unsupported(message)
            }
        }
    })
}

pub(crate) fn matches_predicate(
    index: &IndexMeta,
    payload: &serde_json::Value,
    row_schema: &RowSchema,
) -> Result<bool, CassieError> {
    Midge::payload_matches_scalar_index_predicate(index, payload, row_schema, evaluate_predicate)
}

pub(crate) fn evaluate_stored_expression(
    index_name: &str,
    stored_expression: &str,
    row: &[(String, Value)],
) -> Result<Value, StorageError> {
    evaluate_expression(index_name, stored_expression, row)
        .map_err(|error| StorageError::EngineCallback(Box::new(error)))
}

pub(crate) fn evaluate_stored_predicate(
    index_name: &str,
    stored_predicate: &str,
    row: Vec<(String, Value)>,
) -> Result<bool, StorageError> {
    evaluate_predicate(index_name, stored_predicate, row)
        .map_err(|error| StorageError::EngineCallback(Box::new(error)))
}

fn evaluate_expression(
    index_name: &str,
    raw: &str,
    row: &[(String, Value)],
) -> Result<Value, CassieError> {
    let expression: Expr = serde_json::from_str(raw).map_err(|error| {
        CassieError::Parse(format!("invalid scalar index expression for '{index_name}': {error}"))
    })?;
    filter::evaluate_expr_value(row, &expression, &[], None, &HashMap::new(), None, None)
        .map_err(CassieError::from)
}

fn evaluate_predicate(
    index_name: &str,
    raw: &str,
    row: Vec<(String, Value)>,
) -> Result<bool, CassieError> {
    let predicate: Expr = serde_json::from_str(raw).map_err(|error| {
        CassieError::Parse(format!("invalid scalar index predicate for '{index_name}': {error}"))
    })?;
    let matched = !filter::filter_rows(vec![row], &predicate, &[], None, &HashMap::new(), None)
        .map_err(CassieError::from)?
        .is_empty();
    Ok(matched)
}
