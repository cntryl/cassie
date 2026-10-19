//! `pg_sleep`, `pg_sleep_for` and `pg_sleep_until`, interruptible by
//! cancellation and statement deadlines.
use super::super::QueryError;
use super::{coerce, float, INTERVAL, TIMESTAMPTZ};
use crate::types::extended::temporal::calendar::USECS_PER_DAY;
use crate::types::extended::temporal::session::current_timestamp;
use crate::types::extended::temporal::TemporalSession;
use crate::types::typed::TypedValue;
use crate::types::Value;

const SLICE: std::time::Duration = std::time::Duration::from_millis(5);

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: &TemporalSession,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    let deadline = match (name, args) {
        ("pg_sleep", [seconds]) => {
            let Some(seconds) = float(seconds) else {
                return Some(Ok(Value::Null));
            };
            let micros =
                crate::types::extended::temporal::tokens::round_half_even(seconds * 1_000_000.0);
            current_timestamp().saturating_add(micros)
        }
        ("pg_sleep_for", [span]) => {
            if matches!(span, Value::Null) {
                return Some(Ok(Value::Null));
            }
            match coerce(span, INTERVAL, session) {
                Ok(TypedValue::Interval(span)) => {
                    let days = i64::from(span.months) * 30 + i64::from(span.days);
                    current_timestamp()
                        .saturating_add(days.saturating_mul(USECS_PER_DAY))
                        .saturating_add(span.micros)
                }
                Ok(_) => return Some(Ok(Value::Null)),
                Err(error) => return Some(Err(error)),
            }
        }
        ("pg_sleep_until", [instant]) => {
            if matches!(instant, Value::Null) {
                return Some(Ok(Value::Null));
            }
            match coerce(instant, TIMESTAMPTZ, session) {
                Ok(TypedValue::TimestampTz(instant)) => instant,
                Ok(_) => return Some(Ok(Value::Null)),
                Err(error) => return Some(Err(error)),
            }
        }
        _ => return None,
    };
    // `void` is not NULL: it renders as an empty string, as in PostgreSQL.
    Some(sleep_until(deadline, controls).map(|()| Value::String(String::new())))
}

fn sleep_until(
    deadline: i64,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), QueryError> {
    loop {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let remaining = deadline.saturating_sub(current_timestamp());
        if remaining <= 0 {
            return Ok(());
        }
        let wait = u64::try_from(remaining).unwrap_or(0);
        let wait = std::time::Duration::from_micros(wait).min(SLICE);
        std::thread::sleep(wait);
    }
}
