//! Preserve input identity at scalar cast boundaries.
use super::{
    casts, eval_scalar_with_context, json_concat, numeric_comparison, object_ids, CassieSession,
    DataType, EvalContext, Expr, QueryError, RowAccess, ScalarValue,
};

pub(super) fn eval_cast_expr<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    data_type: &DataType,
    source_type: Option<&DataType>,
    identity: crate::sql::ast::CastIdentity,
    context: EvalContext<'_>,
) -> Result<ScalarValue, QueryError> {
    if let Some(value) = collection_aggregate_text_cast(row, expr, data_type, source_type, context)?
    {
        return Ok(value);
    }
    if data_type.is_json() && source_type.is_some_and(DataType::is_json) {
        let document =
            json_concat::as_json_document(&eval_scalar_with_context(row, expr, context)?)?;
        if *data_type == DataType::Jsonb || source_type == Some(&DataType::Jsonb) {
            if let ScalarValue::Json(text) = document {
                return crate::types::jsonb::canonical(&text)
                    .map(ScalarValue::Json)
                    .map_err(|error| crate::app::CassieError::TypeInput(error).into());
            }
        }
        return Ok(document);
    }
    if let Some(value) = percentile_array_text_cast(row, expr, data_type, context)? {
        return Ok(value);
    }
    if let Expr::NumericLiteral(value) = expr {
        return casts::numeric_literal(
            value,
            data_type,
            identity,
            context.controls.or_else(|| {
                context
                    .session
                    .and_then(CassieSession::statement_execution_controls)
            }),
        );
    }
    let value = eval_scalar_with_context(row, expr, context)?;
    if let (DataType::Array(_), Some(memory)) = (data_type, context.memory) {
        if let ScalarValue::Str(text) | ScalarValue::Json(text) = &value {
            memory.grow(crate::executor::retained_memory::add(
                crate::executor::retained_memory::mul(text.len(), 128)?,
                1024,
            )?)?;
        }
    }
    if let Some(cast) = object_ids::catalog_cast(&value, data_type, source_type, context.session)? {
        return Ok(cast);
    }
    if identity == crate::sql::ast::CastIdentity::Scalar
        && *data_type == DataType::Float
        && crate::sql::binder::generated_float_coercion_case_operand(expr).is_some()
    {
        numeric_comparison::float_scalar(&value)
    } else {
        #[cfg(test)]
        if matches!(data_type, DataType::Array(_)) {
            ARRAY_CAST_CALLS.with(|count| count.set(count.get() + 1));
        }
        casts::scalar(
            &value,
            data_type,
            source_type,
            context.controls.or_else(|| {
                context
                    .session
                    .and_then(CassieSession::statement_execution_controls)
            }),
        )
    }
}

fn collection_aggregate_text_cast<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    data_type: &DataType,
    source_type: Option<&DataType>,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    if !matches!(data_type, DataType::Text | DataType::Varchar { .. })
        || source_type != Some(&DataType::Json)
    {
        return Ok(None);
    }
    let Expr::Function(function) = expr else {
        return Ok(None);
    };
    let Some(crate::sql::functions::ScalarFunction {
        id: crate::sql::functions::FunctionId::CollectionAggregate(kind),
        ..
    }) = crate::sql::functions::function(&function.name)
    else {
        return Ok(None);
    };
    let ScalarValue::Json(text) = eval_scalar_with_context(row, expr, context)? else {
        return Ok(None);
    };
    let document = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let formatted = collection_aggregate_json_text(*kind, document)?;
    Ok(Some(ScalarValue::Str(formatted)))
}

fn collection_aggregate_json_text(
    kind: crate::sql::functions::CollectionAggregate,
    document: serde_json::Value,
) -> Result<String, QueryError> {
    Ok(match kind {
        crate::sql::functions::CollectionAggregate::Json { .. } => match document {
            serde_json::Value::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(|value| {
                        crate::types::jsonb::format(value).unwrap_or_else(|_| value.to_string())
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            value => value.to_string(),
        },
        crate::sql::functions::CollectionAggregate::JsonObject { binary: false, .. } => {
            match document {
                serde_json::Value::Object(fields) => format!(
                    "{{ {} }}",
                    fields
                        .iter()
                        .map(|(key, value)| format!(
                            "{} : {}",
                            serde_json::to_string(key).unwrap_or_default(),
                            crate::types::jsonb::format(value)
                                .unwrap_or_else(|_| value.to_string())
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                value => value.to_string(),
            }
        }
        crate::sql::functions::CollectionAggregate::JsonObject { binary: true, .. } => {
            crate::types::jsonb::format(&document).map_err(crate::app::CassieError::TypeInput)?
        }
        crate::sql::functions::CollectionAggregate::Array => {
            unreachable!("SQL arrays are not JSON aggregates")
        }
    })
}

fn percentile_array_text_cast<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    data_type: &DataType,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    if !matches!(data_type, DataType::Text | DataType::Varchar { .. })
        || !percentile_array_expression(expr)
    {
        return Ok(None);
    }
    let ScalarValue::Json(value) = eval_scalar_with_context(row, expr, context)? else {
        return Ok(None);
    };
    let Ok(array) = serde_json::from_str::<serde_json::Value>(&value) else {
        return Ok(None);
    };
    let Some(formatted) = crate::types::array::format_json_array_text(&array) else {
        return Ok(None);
    };
    casts::scalar(
        &ScalarValue::Str(formatted),
        data_type,
        Some(&DataType::Text),
        context.controls.or_else(|| {
            context
                .session
                .and_then(CassieSession::statement_execution_controls)
        }),
    )
    .map(Some)
}

fn percentile_array_expression(expression: &Expr) -> bool {
    let function = match expression {
        Expr::Function(function) => function.as_ref(),
        Expr::Column(name)
            if name.starts_with("percentile_cont(") || name.starts_with("percentile_disc(") =>
        {
            return true;
        }
        _ => return false,
    };
    let percentile = crate::sql::functions::function(&function.name).is_some_and(|metadata| {
        matches!(
            metadata.id,
            crate::sql::functions::FunctionId::PercentileCont
                | crate::sql::functions::FunctionId::PercentileDisc
        )
    });
    percentile
        && function
            .args
            .first()
            .is_some_and(|argument| match argument {
                Expr::Subquery(subquery) => {
                    matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
                }
                Expr::Cast {
                    data_type: DataType::Array(_),
                    ..
                } => true,
                _ => false,
            })
}

#[cfg(test)]
thread_local! { static ARRAY_CAST_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod array_admission_tests {
    use super::*;
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_source_argument, EvaluationMemory};
    use crate::runtime::QueryExecutionControls;
    use std::time::Instant;

    #[test]
    fn should_admit_native_array_text_casts_before_entering_the_array_parser() {
        // Arrange
        let expression = Expr::Cast {
            expr: Box::new(Expr::StringLiteral(format!("{{{}}}", ["1"; 32].join(",")))),
            data_type: DataType::Array(Box::new(DataType::Int)),
            source_type: Some(DataType::Text),
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
        let row = BatchRow::from_projected_values(Vec::new());
        let functions = std::collections::HashMap::new();

        // Act / Assert
        for (budget, accepted, calls) in [(8_192, false, 0), (32_768, true, 1)] {
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..crate::config::CassieRuntimeLimits::default()
                },
                Instant::now(),
            );
            let memory = EvaluationMemory::new(&controls).expect("array cast owner");
            ARRAY_CAST_CALLS.with(|count| count.set(0));
            let result = evaluate_source_argument(
                &row,
                &expression,
                &[],
                &functions,
                None,
                &controls,
                &memory,
            );
            assert_eq!(result.is_ok(), accepted);
            if !accepted {
                assert!(matches!(
                    result,
                    Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                        _
                    )))
                ));
            }
            assert_eq!(ARRAY_CAST_CALLS.with(std::cell::Cell::get), calls);
            drop(result);
            drop(memory);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
