//! Peer-aware ranking and distribution window functions.

use num_traits::ToPrimitive;

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::sql::ast::WindowFunctionCall;
use crate::types::Value;

use super::{check_timeout, QueryError, WindowExecutionContext, WindowValuePosition};

pub(super) fn evaluate_partition(
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    function_name: &str,
    indices: &mut [usize],
    values: &mut [Value],
    context: &WindowExecutionContext<'_>,
) -> Result<(), QueryError> {
    super::sort_partition(rows, function, indices, context)?;
    let frame = super::effective_window_frame(function);
    let ntile_buckets = if function_name == "ntile" {
        ntile_bucket_count(indices.first().copied(), rows, function, context)?
    } else {
        None
    };
    let mut position = 0;
    let mut dense_rank = 0_i64;
    while position < indices.len() {
        check_timeout(context.controls)?;
        dense_rank = dense_rank.saturating_add(1);
        let peer_key =
            super::window_peer_key(&rows[indices[position]], &function.order_by, context)?;
        let mut peer_end = position + 1;
        while peer_end < indices.len() {
            check_timeout(context.controls)?;
            let candidate =
                super::window_peer_key(&rows[indices[peer_end]], &function.order_by, context)?;
            if candidate != peer_key {
                break;
            }
            peer_end += 1;
        }
        let rank = i64::try_from(position + 1).unwrap_or(i64::MAX);
        for peer_position in position..peer_end {
            check_timeout(context.controls)?;
            let index = indices[peer_position];
            values[index] = super::window_value_at(
                WindowValuePosition {
                    position: peer_position,
                    rank,
                    dense_rank,
                    peer_end,
                    partition_len: indices.len(),
                    ntile_buckets,
                    frame: &frame,
                },
                rows,
                function,
                function_name,
                indices,
                context,
            )?;
        }
        position = peer_end;
    }
    Ok(())
}

pub(super) fn percent_rank_value(
    value_position: WindowValuePosition<'_>,
) -> Result<Value, QueryError> {
    if value_position.partition_len <= 1 {
        return Ok(Value::Float64(0.0));
    }
    let numerator = usize::try_from(value_position.rank - 1)
        .ok()
        .and_then(|value| value.to_f64())
        .ok_or_else(|| CassieError::ResourceLimit("window percent_rank exceeds FLOAT8".into()))?;
    let denominator = (value_position.partition_len - 1)
        .to_f64()
        .ok_or_else(|| CassieError::ResourceLimit("window percent_rank exceeds FLOAT8".into()))?;
    Ok(Value::Float64(numerator / denominator))
}

pub(super) fn cume_dist_value(
    value_position: WindowValuePosition<'_>,
) -> Result<Value, QueryError> {
    let numerator = value_position
        .peer_end
        .to_f64()
        .ok_or_else(|| CassieError::ResourceLimit("window cume_dist exceeds FLOAT8".into()))?;
    let denominator = value_position
        .partition_len
        .to_f64()
        .filter(|value| *value != 0.0)
        .ok_or_else(|| CassieError::ResourceLimit("window cume_dist exceeds FLOAT8".into()))?;
    Ok(Value::Float64(numerator / denominator))
}

pub(super) fn ntile_value_at(value_position: WindowValuePosition<'_>) -> Result<Value, QueryError> {
    let Some(bucket_count) = value_position.ntile_buckets else {
        return Ok(Value::Null);
    };
    ntile_value(
        value_position.position,
        value_position.partition_len,
        bucket_count,
    )
    .map(Value::Int64)
}

fn ntile_bucket_count(
    index: Option<usize>,
    rows: &[BatchRow],
    function: &WindowFunctionCall,
    context: &WindowExecutionContext<'_>,
) -> Result<Option<usize>, QueryError> {
    let Some(index) = index else {
        return Ok(None);
    };
    if let Some(argument) = function.args.first() {
        if let Some(data_type) = crate::executor::array_order::expression_type(
            &rows[index],
            argument,
            context.user_functions,
        ) {
            if !matches!(
                &data_type,
                crate::types::DataType::SmallInt
                    | crate::types::DataType::Int
                    | crate::types::DataType::Null
            ) {
                return Err(ntile_argument_type_error(&data_type));
            }
        }
    }
    let value = super::window_arg_value(Some(index), rows, function, context)?;
    match value {
        Value::Null => Ok(None),
        Value::Int64(buckets) if buckets > 0 => usize::try_from(buckets).map(Some).map_err(|_| {
            CassieError::InvalidParameterValue("argument of ntile must be greater than zero".into())
                .into()
        }),
        Value::Int64(_) => Err(CassieError::InvalidParameterValue(
            "argument of ntile must be greater than zero".into(),
        )
        .into()),
        _ => Err(
            CassieError::InvalidParameterValue("ntile argument must be an integer".into()).into(),
        ),
    }
}

fn ntile_argument_type_error(data_type: &crate::types::DataType) -> QueryError {
    let type_name = match data_type {
        crate::types::DataType::Float => "double precision".to_owned(),
        crate::types::DataType::Int => "integer".to_owned(),
        other => other.type_name(),
    };
    CassieError::Execution(format!("function ntile({type_name}) does not exist")).into()
}

fn ntile_value(
    position: usize,
    partition_len: usize,
    bucket_count: usize,
) -> Result<i64, QueryError> {
    let smaller_bucket_size = partition_len / bucket_count;
    let larger_bucket_count = partition_len % bucket_count;
    let larger_bucket_size = smaller_bucket_size.saturating_add(1);
    let larger_rows = larger_bucket_size
        .checked_mul(larger_bucket_count)
        .ok_or_else(|| CassieError::ResourceLimit("NTILE bucket row count overflow".into()))?;
    let bucket = if position < larger_rows {
        position / larger_bucket_size + 1
    } else {
        (position - larger_rows)
            .checked_div(smaller_bucket_size)
            .map_or(position + 1, |offset| larger_bucket_count + offset + 1)
    };
    i64::try_from(bucket)
        .map_err(|_| CassieError::ResourceLimit("NTILE bucket exceeds INTEGER".into()).into())
}
