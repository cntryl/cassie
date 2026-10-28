//! Existing fixed-duration time buckets.
use super::{QueryError, Value};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

pub(super) fn evaluate_time_bucket(args: &[Value]) -> Result<Value, QueryError> {
    if !(2..=3).contains(&args.len()) {
        return Err(QueryError::General(format!(
            "time_bucket requires 2 or 3 args, got {}",
            args.len()
        )));
    }
    if args.iter().any(|arg| matches!(arg, Value::Null)) {
        return Ok(Value::Null);
    }

    let width_ns = duration_width_ns(&args[0])?;
    let timestamp_ns = timestamp_arg_ns("time_bucket", &args[1])?;
    let origin_ns = if args.len() == 3 {
        timestamp_arg_ns("time_bucket", &args[2])?
    } else {
        0
    };
    let delta = timestamp_ns
        .checked_sub(origin_ns)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket_index = floor_div(delta, width_ns);
    let bucket_offset = bucket_index
        .checked_mul(width_ns)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket_ns = origin_ns
        .checked_add(bucket_offset)
        .ok_or_else(|| QueryError::General("time_bucket timestamp overflow".to_string()))?;
    let bucket = OffsetDateTime::from_unix_timestamp_nanos(bucket_ns)
        .map_err(|_| QueryError::General("time_bucket timestamp overflow".to_string()))?
        .to_offset(UtcOffset::UTC);
    let formatted = bucket
        .format(&Rfc3339)
        .map_err(|error| QueryError::General(error.to_string()))?;
    Ok(Value::String(formatted))
}

fn duration_width_ns(value: &Value) -> Result<i128, QueryError> {
    let Value::String(raw) = value else {
        return Err(QueryError::General(
            "time_bucket width must be a duration string".to_string(),
        ));
    };
    let (amount, unit) = parse_duration_parts(raw)?;
    let multiplier = match unit.as_str() {
        "ns" | "nanosecond" | "nanoseconds" => 1,
        "us" | "microsecond" | "microseconds" => 1_000,
        "ms" | "millisecond" | "milliseconds" => 1_000_000,
        "s" | "sec" | "second" | "seconds" => 1_000_000_000,
        "m" | "min" | "minute" | "minutes" => 60_000_000_000,
        "h" | "hr" | "hour" | "hours" => 3_600_000_000_000,
        "d" | "day" | "days" => 86_400_000_000_000,
        "w" | "week" | "weeks" => 604_800_000_000_000,
        "month" | "months" | "year" | "years" => {
            return Err(QueryError::General(
                "time_bucket width cannot use calendar-dependent units".to_string(),
            ));
        }
        _ => {
            return Err(QueryError::General(format!(
                "invalid time_bucket width unit '{unit}'"
            )));
        }
    };
    amount
        .checked_mul(multiplier)
        .filter(|width| *width > 0)
        .ok_or_else(|| QueryError::General("time_bucket width overflow".to_string()))
}

fn parse_duration_parts(raw: &str) -> Result<(i128, String), QueryError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(QueryError::General("invalid time_bucket width".to_string()));
    }
    let split_at = value
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '-' || ch == '+'))
        .unwrap_or(value.len());
    let number = value[..split_at].trim();
    let unit = value[split_at..].trim().to_ascii_lowercase();
    if number.is_empty() || unit.is_empty() {
        return Err(QueryError::General("invalid time_bucket width".to_string()));
    }
    let amount = number
        .parse::<i128>()
        .map_err(|_| QueryError::General("invalid time_bucket width amount".to_string()))?;
    if amount <= 0 {
        return Err(QueryError::General(
            "time_bucket width must be positive".to_string(),
        ));
    }
    Ok((amount, unit))
}

fn timestamp_arg_ns(name: &str, value: &Value) -> Result<i128, QueryError> {
    if let Value::Typed(typed) = value {
        if let Some(raw) = crate::types::extended::temporal_legacy_text(typed) {
            return parse_timestamp_utc(&raw);
        }
    }
    let Value::String(raw) = value else {
        return Err(QueryError::General(format!(
            "function '{name}' expects timestamp input"
        )));
    };
    parse_timestamp_utc(raw)
}

fn parse_timestamp_utc(raw: &str) -> Result<i128, QueryError> {
    crate::types::temporal::parse_timestamp_utc(raw.trim())
        .map(OffsetDateTime::unix_timestamp_nanos)
        .map_err(|_| QueryError::General("invalid time_bucket timestamp".to_string()))
}

fn floor_div(value: i128, divisor: i128) -> i128 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && ((remainder > 0) != (divisor > 0)) {
        quotient - 1
    } else {
        quotient
    }
}
