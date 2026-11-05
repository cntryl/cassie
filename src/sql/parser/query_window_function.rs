//! Validate window calls without duplicating a paired aggregate's FILTER.
use super::SqlError;
use crate::sql::ast::{AggregateQuantifier, Expr, FunctionCall};

pub(super) fn prepare(mut function: FunctionCall) -> Result<FunctionCall, SqlError> {
    let name = function.name.to_ascii_lowercase();
    let local = name.strip_prefix("pg_catalog.").unwrap_or(&name);
    let aggregate = crate::sql::functions::aggregate_window_function(&name).is_some();
    if aggregate {
        if function.is_distinct() || !function.order_by.is_empty() {
            return Err(SqlError::unsupported(
                "DISTINCT and aggregate-local ORDER BY are not supported on window aggregates"
                    .into(),
            ));
        }
        if local == "count"
            && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*")
        {
            function.args = vec![Expr::IntegerLiteral(1)];
        }
        function.quantifier = AggregateQuantifier::Default;
    } else if function.has_modifiers() {
        return Err(SqlError::unsupported(
            "aggregate modifiers on window functions are not yet supported".into(),
        ));
    }
    validate_window_function(&function, local, aggregate)?;
    Ok(function)
}

fn validate_window_function(
    function: &FunctionCall,
    function_name: &str,
    aggregate_window: bool,
) -> Result<(), SqlError> {
    let registered_window_function: Option<&crate::sql::functions::WindowFunction> =
        crate::sql::functions::window_function(function_name);
    if !matches!(
        function_name,
        "row_number"
            | "rank"
            | "dense_rank"
            | "percent_rank"
            | "cume_dist"
            | "ntile"
            | "lag"
            | "lead"
            | "first_value"
            | "last_value"
            | "nth_value"
            | "count"
            | "sum"
            | "bool_and"
            | "bool_or"
            | "var_pop"
            | "var_samp"
            | "variance"
            | "stddev_pop"
            | "stddev_samp"
            | "stddev"
    ) && !aggregate_window
        && registered_window_function.is_none()
    {
        return Err(SqlError::new(format!(
            "unsupported window function '{}'",
            function.name
        )));
    }
    if matches!(
        function_name,
        "row_number" | "rank" | "dense_rank" | "percent_rank" | "cume_dist"
    ) && !function.args.is_empty()
    {
        return Err(SqlError::new(format!(
            "{} window function expects no args",
            function.name
        )));
    }
    if function_name == "ntile" && function.args.len() != 1 {
        return Err(SqlError::new(
            "ntile window function expects one arg".into(),
        ));
    }
    if matches!(function_name, "first_value" | "last_value") && function.args.len() != 1 {
        return Err(SqlError::new(format!(
            "{} window function expects one arg",
            function.name
        )));
    }
    if let Some(metadata) = registered_window_function {
        if !metadata.arity.matches(function.args.len()) {
            let expected = match metadata.id {
                crate::sql::functions::FunctionId::Lag
                | crate::sql::functions::FunctionId::Lead => "one to three args",
                crate::sql::functions::FunctionId::NthValue => "two args",
                _ => unreachable!("only registered value window functions use this path"),
            };
            return Err(SqlError::new(format!(
                "{} window function expects {expected}",
                function.name
            )));
        }
    }
    if aggregate_window {
        let metadata = crate::sql::functions::aggregate_window_function(&function.name)
            .expect("registered window aggregate");
        if !metadata.arity.matches(function.args.len())
            || function
                .args
                .iter()
                .any(|argument| matches!(argument, Expr::Column(name) if name == "*"))
        {
            return Err(SqlError::undefined_function(format!(
                "function {}(...) does not exist",
                function.name
            )));
        }
    }
    Ok(())
}
