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
    if matches!(data_type, DataType::Text | DataType::Varchar { .. })
        && percentile_array_expression(expr)
    {
        let value = eval_scalar_with_context(row, expr, context)?;
        if let ScalarValue::Json(value) = value {
            if let Ok(array) = serde_json::from_str::<serde_json::Value>(&value) {
                if let Some(formatted) = crate::types::array::format_json_array_text(&array) {
                    return casts::scalar(
                        &ScalarValue::Str(formatted),
                        data_type,
                        Some(&DataType::Text),
                        context.controls.or_else(|| {
                            context
                                .session
                                .and_then(CassieSession::statement_execution_controls)
                        }),
                    );
                }
            }
        }
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
    if let Some(cast) = object_ids::catalog_cast(&value, data_type, source_type, context.session)? {
        return Ok(cast);
    }
    if identity == crate::sql::ast::CastIdentity::Scalar
        && *data_type == DataType::Float
        && crate::sql::binder::generated_float_coercion_case_operand(expr).is_some()
    {
        numeric_comparison::float_scalar(&value)
    } else {
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
