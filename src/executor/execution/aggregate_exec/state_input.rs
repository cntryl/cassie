//! Argument and FILTER evaluation shared by aggregate transitions.
use std::collections::HashMap;

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::executor::semantic::SemanticValue;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::Value;

use super::retained_change::RetainedChange;
use super::state::{has_bytea_carrier, is_string_aggregate, AggregateAccumulator};
use super::{AggregateExecutionContext, QueryError};

pub(super) struct AggregateValueContext<'a> {
    pub(super) params: &'a [Value],
    pub(super) search_context: Option<&'a filter::SearchContext>,
    pub(super) user_functions: &'a HashMap<String, FunctionMeta>,
    pub(super) session: Option<&'a CassieSession>,
}

fn value_context<'context, 'data>(
    context: &'context AggregateExecutionContext<'data>,
) -> AggregateValueContext<'context>
where
    'data: 'context,
{
    AggregateValueContext {
        params: context.params,
        search_context: context.search_context,
        user_functions: context.user_functions,
        session: context.session,
    }
}

pub(super) fn selected_row_input(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    evaluate_selected_input(function, row, &value_context(context))
}

pub(super) fn selected_row_argument(
    function: &FunctionCall,
    index: usize,
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<Value, QueryError> {
    evaluate_argument(function, index, row, &value_context(context))
}

pub(super) fn row_filter_matches(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateExecutionContext<'_>,
) -> Result<bool, QueryError> {
    filter_matches(function, row, &value_context(context))
}

pub(super) fn selected_input(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateValueContext<'_>,
) -> Result<Option<Value>, QueryError> {
    if !filter_matches(function, row, context)? {
        return Ok(None);
    }
    evaluate_selected_input(function, row, context).map(Some)
}

fn filter_matches(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateValueContext<'_>,
) -> Result<bool, QueryError> {
    if let Some(predicate) = &function.filter {
        match filter::evaluate_expr_value(
            row,
            predicate,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )? {
            Value::Bool(true) => {}
            Value::Bool(false) | Value::Null => return Ok(false),
            _ => {
                return Err(QueryError::General(
                    "FILTER requires BOOLEAN or SQL NULL".into(),
                ));
            }
        }
    }
    Ok(true)
}

fn evaluate_selected_input(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateValueContext<'_>,
) -> Result<Value, QueryError> {
    let star = matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*");
    let value = if star {
        Value::Int64(1)
    } else if let Some(value) = function
        .args
        .first()
        .and_then(|expr| stored_json_column_value(row, expr))
    {
        value
    } else {
        evaluate_input(function, row, context)?.unwrap_or(Value::Null)
    };
    Ok(value)
}

pub(super) fn evaluate_argument(
    function: &FunctionCall,
    index: usize,
    row: &BatchRow,
    context: &AggregateValueContext<'_>,
) -> Result<Value, QueryError> {
    let Some(expression) = function.args.get(index) else {
        return Err(super::state::invalid_string_aggregate_input(function));
    };
    if index == 0 {
        return evaluate_selected_input(function, row, context);
    }
    filter::evaluate_expr_value(
        row,
        expression,
        context.params,
        context.search_context,
        context.user_functions,
        context.session,
        None,
    )
}

fn evaluate_input(
    function: &FunctionCall,
    row: &BatchRow,
    context: &AggregateValueContext<'_>,
) -> Result<Option<Value>, QueryError> {
    let Some(expression) = function.args.first() else {
        return Ok(None);
    };
    filter::evaluate_expr_value(
        row,
        expression,
        context.params,
        context.search_context,
        context.user_functions,
        context.session,
        None,
    )
    .map(Some)
}

/// Bare JSON-valued columns bypass scalar evaluation, which otherwise flattens them to text.
pub(super) fn stored_json_column_value(row: &BatchRow, expr: &Expr) -> Option<Value> {
    let Expr::Column(name) = expr else {
        return None;
    };
    match row.get(name) {
        Some(value @ Value::Json(_)) => Some(value.clone()),
        _ => None,
    }
}

fn has_bytea_pair(value: &Value, delimiter: Option<&Value>) -> bool {
    has_bytea_carrier(value) && delimiter.is_some_and(has_bytea_carrier)
}

impl AggregateAccumulator {
    pub(super) fn update(
        &mut self,
        function: &FunctionCall,
        row: &BatchRow,
        params: &[Value],
        search_context: Option<&filter::SearchContext>,
        user_functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<RetainedChange, QueryError> {
        Ok(self
            .update_selected(
                function,
                row,
                params,
                search_context,
                user_functions,
                session,
            )?
            .unwrap_or_default())
    }

    pub(super) fn update_selected(
        &mut self,
        function: &FunctionCall,
        row: &BatchRow,
        params: &[Value],
        search_context: Option<&filter::SearchContext>,
        user_functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<Option<RetainedChange>, QueryError> {
        let value_context = AggregateValueContext {
            params,
            search_context,
            user_functions,
            session,
        };
        let Some(value) = selected_input(function, row, &value_context)? else {
            return Ok(None);
        };
        let second_argument = if matches!(
            self,
            Self::StringAgg { .. } | Self::Regression(_) | Self::Collection { .. }
        ) {
            Some(evaluate_argument(function, 1, row, &value_context)?)
        } else {
            None
        };
        let key = if matches!(self, Self::MinMax { .. }) {
            function
                .args
                .first()
                .map(|expr| crate::executor::array_order::key(row, expr, &value, user_functions))
                .filter(|key| {
                    matches!(
                        key,
                        SemanticValue::Array(_) | SemanticValue::SqlArray { .. }
                    )
                })
        } else {
            None
        };
        let bytea = is_string_aggregate(function)
            && (function.args.first().is_some_and(|argument| {
                crate::executor::array_order::expression_type(row, argument, user_functions)
                    == Some(crate::types::DataType::Bytea)
            }) || has_bytea_pair(&value, second_argument.as_ref()));
        let change = if bytea {
            self.update_values_with_bytea(function, value, key, second_argument, true)
        } else {
            self.update_values(function, value, key, second_argument)
        }?;
        Ok(Some(change))
    }
}
