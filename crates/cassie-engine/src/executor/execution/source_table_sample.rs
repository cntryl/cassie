//! Row selection for the selected PostgreSQL TABLESAMPLE methods.

use super::{
    check_timeout, execute_query_source, filter, BatchRow, CteContext, Expr, QueryError,
    QuerySource, SourceExecution, SourceExecutionEnv, Value,
};
use crate::sql::ast::TableSampleMethod;
use crate::types::typed::TypedValue;

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    method: TableSampleMethod,
    rate: &Expr,
    repeatable: Option<&Expr>,
    cte_context: &mut CteContext,
    qualify: bool,
) -> SourceExecution {
    check_timeout(env.controls)?;
    let empty = BatchRow::from_projected_values(Vec::new());
    let rate = round_to_float4(evaluate_numeric(env, rate, &empty)?)?;
    if !rate.is_finite() || !(0.0..=100.0).contains(&rate) {
        return Err(invalid_percentage());
    }
    let seed = match repeatable {
        Some(expression) => evaluate_numeric(env, expression, &empty)?,
        None => random_seed(),
    };
    let (batches, text_fields) =
        execute_query_source(env, source, cte_context, qualify, None, None)?;
    let mut ordinal = 0_u32;
    let rows_per_block = u32::try_from(super::batch::DEFAULT_BATCH_SIZE)
        .unwrap_or(u32::MAX)
        .max(1);
    let mut sampled = crate::runtime::accounted::AccountedVec::try_new(env.controls)?;
    let threshold = ((f64::from(u32::MAX) + 1.0) * rate / 100.0).round_ties_even();
    for row in batches.into_iter().flatten() {
        check_timeout(env.controls)?;
        ordinal = ordinal.checked_add(1).ok_or_else(|| {
            crate::app::CassieError::ResourceLimit(
                "TABLESAMPLE row ordinal exceeds the supported range".into(),
            )
        })?;
        let block = (ordinal - 1) / rows_per_block;
        let selected = match method {
            TableSampleMethod::Bernoulli => {
                if is_zero_rate(rate) {
                    false
                } else if is_full_rate(rate) {
                    true
                } else {
                    let offset = (ordinal - 1) % rows_per_block + 1;
                    sample_hash_three([block, offset, sample_seed(seed)]) < threshold
                }
            }
            TableSampleMethod::System => {
                if is_zero_rate(rate) {
                    false
                } else if is_full_rate(rate) {
                    true
                } else {
                    sample_hash_two([block, sample_seed(seed)]) < threshold
                }
            }
        };
        if selected {
            sampled.try_push_with(0, || row)?;
        }
    }
    check_timeout(env.controls)?;
    let (mut sampled, reservation) = sampled.into_parts();
    let reservation = std::sync::Arc::new(reservation);
    for row in &mut sampled {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.retain_operator_memory(env.controls, std::sync::Arc::clone(&reservation))?;
    }
    Ok((vec![sampled], text_fields))
}

fn is_zero_rate(rate: f64) -> bool {
    rate.abs().to_bits() == 0.0_f64.to_bits()
}

fn is_full_rate(rate: f64) -> bool {
    rate.to_bits() == 100.0_f64.to_bits()
}

fn evaluate_numeric(
    env: &SourceExecutionEnv<'_>,
    expression: &Expr,
    row: &BatchRow,
) -> Result<f64, QueryError> {
    let value = filter::evaluate_expr_value(
        row,
        expression,
        env.params,
        None,
        env.user_functions,
        env.session,
        None,
    )?;
    match value {
        Value::Int64(value) => value.to_string().parse::<f64>().map_err(|_| {
            crate::app::CassieError::InvalidParameterValue(
                "TABLESAMPLE argument must be numeric".into(),
            )
            .into()
        }),
        Value::Float64(value) => Ok(value),
        Value::Typed(TypedValue::Real(value)) => Ok(f64::from(value)),
        Value::Typed(TypedValue::Numeric(value)) => Ok(value.to_f64()),
        Value::Null => Err(crate::app::CassieError::InvalidParameterValue(
            "TABLESAMPLE argument must not be null".into(),
        )
        .into()),
        _ => Err(crate::app::CassieError::InvalidParameterValue(
            "TABLESAMPLE argument must be numeric".into(),
        )
        .into()),
    }
}

fn round_to_float4(value: f64) -> Result<f64, QueryError> {
    value
        .to_string()
        .parse::<f32>()
        .map(f64::from)
        .map_err(|_| invalid_percentage())
}

fn invalid_percentage() -> QueryError {
    crate::app::CassieError::Diagnostic(Box::new(crate::app::DiagnosticError::new(
        "2202H",
        "TABLESAMPLE percentage must be between 0 and 100",
    )))
    .into()
}

fn random_seed() -> f64 {
    let mut bytes = [0_u8; 8];
    if getrandom::fill(&mut bytes).is_ok() {
        return f64::from_bits(u64::from_ne_bytes(bytes));
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let fallback = nanos ^ u128::from(std::process::id());
    f64::from_bits(u64::try_from(fallback).unwrap_or_default())
}

fn sample_seed(seed: f64) -> u32 {
    if seed == 0.0 {
        return 0;
    }
    let seed = if seed.is_nan() { f64::NAN } else { seed };
    let hash = crate::executor::filter::hash_functions::hash_bytes(&seed.to_le_bytes(), 0);
    hash.1
}

fn sample_hash_two(words: [u32; 2]) -> f64 {
    let mut bytes = [0_u8; 8];
    bytes[..4].copy_from_slice(&words[0].to_le_bytes());
    bytes[4..].copy_from_slice(&words[1].to_le_bytes());
    let hash = crate::executor::filter::hash_functions::hash_bytes(&bytes, 0);
    f64::from(hash.1)
}

fn sample_hash_three(words: [u32; 3]) -> f64 {
    let mut bytes = [0_u8; 12];
    bytes[..4].copy_from_slice(&words[0].to_le_bytes());
    bytes[4..8].copy_from_slice(&words[1].to_le_bytes());
    bytes[8..].copy_from_slice(&words[2].to_le_bytes());
    let hash = crate::executor::filter::hash_functions::hash_bytes(&bytes, 0);
    f64::from(hash.1)
}
