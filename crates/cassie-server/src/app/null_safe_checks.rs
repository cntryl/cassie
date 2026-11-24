//! Evaluate prepared simple CHECKs using the query comparator and request owner.
use crate::app::CassieError;
use crate::catalog::ConstraintCheck;
use crate::executor::filter::{evaluate_expr_value, evaluate_source_argument, EvaluationMemory};
use crate::sql::ast::Expr;
use crate::types::{DataType, Value};
use std::collections::HashMap;

pub(super) fn evaluate(
    value: &serde_json::Value,
    check: &ConstraintCheck,
    expression: &Expr,
    is_sql_null: bool,
) -> Result<bool, CassieError> {
    let controls = crate::midge::adapter::PublicationControlScope::current_query_controls();
    let memory = controls
        .as_ref()
        .map(EvaluationMemory::new)
        .transpose()
        .map_err(CassieError::from)?;
    if let Some(memory) = &memory {
        let bytes = crate::runtime::accounted::json::retained_bytes(value)?;
        memory
            .grow(crate::executor::retained_memory::add(
                crate::executor::retained_memory::mul(bytes, 4)?,
                crate::executor::retained_memory::add(check.field.len(), 512)?,
            )?)
            .map_err(CassieError::from)?;
    }
    let Expr::Binary { left, .. } = expression else {
        return Err(CassieError::Unsupported(
            "CHECK must remain a simple comparison".into(),
        ));
    };
    let data_type = crate::sql::check_comparison::field_type(left, &check.field)
        .ok_or_else(|| CassieError::Unsupported("CHECK must retain its owning field".into()))?;
    let row = vec![(
        check.field.clone(),
        if is_sql_null {
            Value::Null
        } else {
            query_value(value, data_type)?
        },
    )];
    let functions = HashMap::new();
    let result = if let (Some(controls), Some(memory)) = (&controls, &memory) {
        evaluate_source_argument(&row, expression, &[], &functions, None, controls, memory)
    } else {
        evaluate_expr_value(&row, expression, &[], None, &functions, None, None)
    }
    .map_err(CassieError::from)?;
    match result {
        Value::Bool(value) => Ok(value),
        _ => Err(CassieError::InvalidQuery(
            "null-safe CHECK must produce BOOLEAN".into(),
        )),
    }
}
fn query_value(value: &serde_json::Value, data_type: &DataType) -> Result<Value, CassieError> {
    if data_type.is_json() {
        return Ok(Value::Json(value.clone()));
    }
    if let DataType::Extended(kind) = data_type {
        return kind
            .from_json(value)
            .map(Value::Typed)
            .map_err(CassieError::TypeInput);
    }
    if let DataType::Array(element) = data_type {
        let array = if value.is_object() {
            crate::types::SqlArray::from_storage_json(value)
        } else {
            crate::types::SqlArray::from_nested_json_typed(value, element)
        };
        return array.map(Value::Array).map_err(CassieError::InvalidQuery);
    }
    Ok(match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::String(value) => Value::String(value.clone()),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(Value::Int64)
            .or_else(|| value.as_f64().map(Value::Float64))
            .unwrap_or_else(|| Value::Json(serde_json::Value::Number(value.clone()))),
        other => Value::Json(other.clone()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_admit_check_inputs_before_owned_conversion() {
        // Arrange
        let expression =
            crate::sql::parser::parse_expression("CAST(n AS TEXT) IS NOT DISTINCT FROM 'a'")
                .expect("simple check");
        let check = ConstraintCheck {
            field: "n".into(),
            operator: crate::catalog::ConstraintOperator::IsNotDistinctFrom,
            value: "a".into(),
            bound_expression: Some(Box::new(expression.clone())),
        };
        for (budget, accepted) in [(0, false), (4096, true)] {
            let controls = crate::runtime::QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..Default::default()
                },
                std::time::Instant::now(),
            );
            let scope = crate::midge::adapter::PublicationControlScope::enter(Some(&controls));
            // Act
            let result = evaluate(&serde_json::json!("a"), &check, &expression, false);
            // Assert
            if accepted {
                assert!(result.expect("admitted check"));
            } else {
                assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
                assert_eq!(controls.peak_query_memory_bytes(), 0);
            }
            drop(scope);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
