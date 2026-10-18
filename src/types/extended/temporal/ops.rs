//! Date/time operators: `timestamp_pl_interval`, `date_mi`, `interval_mul`,
//! cross-type comparison and the other `+ - * /` overloads.
use std::cmp::Ordering;

use super::calendar::{
    date2j, days_in_month, fields_to_timestamp, is_valid_date, is_valid_timestamp, j2date,
    timestamp_to_fields, DATE_NOBEGIN, DATE_NOEND, MONTHS_PER_YEAR, POSTGRES_EPOCH_JDATE,
    TIMESTAMP_END_JULIAN, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND, USECS_PER_DAY, USECS_PER_SEC,
};
use super::zone::Zone;
use super::{range_error, Interval, TemporalSession, TimeTz};
use crate::types::extended::TypeInputError;
use crate::types::typed::TypedValue;

/// A non-temporal operand of a mixed operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Int(i64),
    Float(f64),
}

impl Number {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => num_traits::ToPrimitive::to_f64(&value).unwrap_or(0.0),
            Self::Float(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Typed(TypedValue),
    Number(Number),
}

/// The result of a temporal operator.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Typed(TypedValue),
    Int(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
}

fn timestamp_range() -> TypeInputError {
    range_error("timestamp out of range")
}

fn interval_range() -> TypeInputError {
    range_error("interval out of range")
}

fn date_range() -> TypeInputError {
    range_error("date out of range")
}

const fn is_infinite_timestamp(value: i64) -> bool {
    value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND
}

/// `date2timestamp`.
///
/// # Errors
/// Returns `22008` beyond the timestamp range.
pub fn date_to_timestamp(date: i32) -> Result<i64, TypeInputError> {
    match date {
        DATE_NOBEGIN => Ok(TIMESTAMP_NOBEGIN),
        DATE_NOEND => Ok(TIMESTAMP_NOEND),
        date if i64::from(date) >= TIMESTAMP_END_JULIAN - i64::from(POSTGRES_EPOCH_JDATE) => {
            Err(range_error("date out of range for timestamp"))
        }
        date => Ok(i64::from(date) * USECS_PER_DAY),
    }
}

fn add_months(fields: &mut super::calendar::Fields, months: i32) -> Result<(), TypeInputError> {
    let mut month = fields
        .month
        .checked_add(months)
        .ok_or_else(timestamp_range)?;
    if month > MONTHS_PER_YEAR {
        fields.year += (month - 1) / MONTHS_PER_YEAR;
        month = (month - 1) % MONTHS_PER_YEAR + 1;
    } else if month < 1 {
        fields.year += month / MONTHS_PER_YEAR - 1;
        month = month % MONTHS_PER_YEAR + MONTHS_PER_YEAR;
    }
    fields.month = month;
    fields.day = fields.day.min(days_in_month(fields.year, fields.month));
    Ok(())
}

fn add_days(fields: &mut super::calendar::Fields, days: i32) -> Result<(), TypeInputError> {
    let julian = date2j(fields.year, fields.month, fields.day)
        .checked_add(i64::from(days))
        .filter(|julian| *julian >= 0 && *julian <= i64::from(i32::MAX))
        .ok_or_else(timestamp_range)?;
    let (year, month, day) = j2date(julian);
    fields.year = year;
    fields.month = month;
    fields.day = day;
    Ok(())
}

/// `timestamp_pl_interval`; `zone` adds months and days in local time for
/// `timestamptz` (`timestamptz_pl_interval_internal`).
///
/// # Errors
/// Returns `22008` when the result leaves the timestamp range.
pub fn timestamp_plus_interval(
    value: i64,
    span: &Interval,
    zone: Option<&Zone>,
) -> Result<i64, TypeInputError> {
    if span.is_infinite() {
        let positive = *span == Interval::INFINITY;
        if (positive && value == TIMESTAMP_NOBEGIN) || (!positive && value == TIMESTAMP_NOEND) {
            return Err(timestamp_range());
        }
        return Ok(if positive {
            TIMESTAMP_NOEND
        } else {
            TIMESTAMP_NOBEGIN
        });
    }
    if is_infinite_timestamp(value) {
        return Ok(value);
    }
    let mut value = value;
    if span.months != 0 || span.days != 0 {
        let local = zone.map_or(value, |zone| zone.utc_to_local(value));
        let mut fields = timestamp_to_fields(local).ok_or_else(timestamp_range)?;
        if span.months != 0 {
            add_months(&mut fields, span.months)?;
        }
        if span.days != 0 {
            add_days(&mut fields, span.days)?;
        }
        let local = fields_to_timestamp(&fields, None).ok_or_else(timestamp_range)?;
        value = zone.map_or(local, |zone| zone.local_to_utc(local));
    }
    let value = value.checked_add(span.micros).ok_or_else(timestamp_range)?;
    if !is_valid_timestamp(value) {
        return Err(timestamp_range());
    }
    Ok(value)
}

/// `timestamp_mi`: the difference, justified into days and hours.
///
/// # Errors
/// Returns `22008` for differences of equal infinities or overflow.
pub fn timestamp_difference(left: i64, right: i64) -> Result<Interval, TypeInputError> {
    match (left, right) {
        (TIMESTAMP_NOBEGIN, TIMESTAMP_NOBEGIN) | (TIMESTAMP_NOEND, TIMESTAMP_NOEND) => {
            return Err(interval_range());
        }
        (TIMESTAMP_NOBEGIN, _) | (_, TIMESTAMP_NOEND) => return Ok(Interval::NEG_INFINITY),
        (TIMESTAMP_NOEND, _) | (_, TIMESTAMP_NOBEGIN) => return Ok(Interval::INFINITY),
        _ => {}
    }
    let micros = left.checked_sub(right).ok_or_else(interval_range)?;
    Interval::new(0, 0, micros)
        .justify_hours()
        .map_err(|_| interval_range())
}

fn date_plus_days(date: i32, days: i64) -> Result<i32, TypeInputError> {
    if date == DATE_NOBEGIN || date == DATE_NOEND {
        return Ok(date);
    }
    let result = i64::from(date).checked_add(days).ok_or_else(date_range)?;
    if !is_valid_date(result) {
        return Err(date_range());
    }
    i32::try_from(result).map_err(|_| date_range())
}

fn time_plus(time: i64, micros: i64) -> i64 {
    (time + micros % USECS_PER_DAY).rem_euclid(USECS_PER_DAY)
}

fn interval_factor(
    op: ArithmeticOp,
    span: &Interval,
    factor: f64,
) -> Result<TypedValue, TypeInputError> {
    let result = if op == ArithmeticOp::Mul {
        span.mul(factor)
    } else {
        if factor == 0.0 {
            return Err(TypeInputError {
                sqlstate: "22012",
                message: "division by zero".to_string(),
            });
        }
        span.div(factor)
    };
    result
        .map(TypedValue::Interval)
        .map_err(|_| interval_range())
}

/// Applies a temporal arithmetic overload; `None` when no overload applies.
#[must_use]
pub fn arithmetic(
    left: &Operand,
    op: ArithmeticOp,
    right: &Operand,
    session: &TemporalSession,
) -> Option<Result<Outcome, TypeInputError>> {
    use ArithmeticOp::{Add, Div, Mul, Sub};
    use Operand::{Number as N, Typed as T};
    use TypedValue::{
        Date, Interval as I, Time, TimeTz as Tz, Timestamp as Ts, TimestampTz as Tstz,
    };
    let typed = |value: Result<TypedValue, TypeInputError>| Some(value.map(Outcome::Typed));
    let integer = |number: &Number| match number {
        Number::Int(value) => Some(*value),
        Number::Float(_) => None,
    };
    match (left, op, right) {
        (T(Date(date)), Add, N(days)) | (N(days), Add, T(Date(date))) => {
            typed(date_plus_days(*date, integer(days)?).map(Date))
        }
        (T(Date(date)), Sub, N(days)) => typed(
            integer(days)?
                .checked_neg()
                .ok_or_else(date_range)
                .and_then(|days| date_plus_days(*date, days))
                .map(Date),
        ),
        (T(Date(left)), Sub, T(Date(right))) => Some(date_difference(*left, *right)),
        (T(Date(date)), Add | Sub, T(I(span))) | (T(I(span)), Add, T(Date(date))) => {
            let span = if op == Sub {
                span.negate().ok()?
            } else {
                *span
            };
            typed(
                date_to_timestamp(*date)
                    .and_then(|value| timestamp_plus_interval(value, &span, None))
                    .map(Ts),
            )
        }
        (T(Date(date)), Add, T(Time(time))) | (T(Time(time)), Add, T(Date(date))) => typed(
            date_to_timestamp(*date)
                .and_then(|value| {
                    if is_infinite_timestamp(value) {
                        Ok(value)
                    } else {
                        value
                            .checked_add(*time)
                            .filter(|value| is_valid_timestamp(*value))
                            .ok_or_else(timestamp_range)
                    }
                })
                .map(Ts),
        ),
        (T(Date(date)), Add, T(Tz(time))) | (T(Tz(time)), Add, T(Date(date))) => typed(
            date_to_timestamp(*date)
                .map(|value| value + time.time + i64::from(time.zone) * USECS_PER_SEC)
                .map(Tstz),
        ),
        (T(Ts(value)), Add | Sub, T(I(span))) | (T(I(span)), Add, T(Ts(value))) => {
            let span = if op == Sub {
                span.negate().ok()?
            } else {
                *span
            };
            typed(timestamp_plus_interval(*value, &span, None).map(Ts))
        }
        (T(Tstz(value)), Add | Sub, T(I(span))) | (T(I(span)), Add, T(Tstz(value))) => {
            let span = if op == Sub {
                span.negate().ok()?
            } else {
                *span
            };
            typed(timestamp_plus_interval(*value, &span, Some(&session.zone)).map(Tstz))
        }
        (T(Ts(left)), Sub, T(Ts(right))) | (T(Tstz(left)), Sub, T(Tstz(right))) => {
            typed(timestamp_difference(*left, *right).map(I))
        }
        (T(Time(time)), Add | Sub, T(I(span))) | (T(I(span)), Add, T(Time(time))) => {
            let micros = if op == Sub { -span.micros } else { span.micros };
            typed(Ok(Time(time_plus(*time, micros))))
        }
        (T(Tz(value)), Add | Sub, T(I(span))) | (T(I(span)), Add, T(Tz(value))) => {
            let micros = if op == Sub { -span.micros } else { span.micros };
            typed(Ok(Tz(TimeTz {
                time: time_plus(value.time, micros),
                zone: value.zone,
            })))
        }
        (T(Time(left)), Sub, T(Time(right))) => typed(Ok(I(Interval::new(0, 0, left - right)))),
        (T(I(left)), Add, T(I(right))) => {
            typed(left.add(right).map(I).map_err(|_| interval_range()))
        }
        (T(I(left)), Sub, T(I(right))) => {
            typed(left.sub(right).map(I).map_err(|_| interval_range()))
        }
        (T(I(span)), Mul | Div, N(number)) => typed(interval_factor(op, span, number.as_f64())),
        (N(number), Mul, T(I(span))) => typed(interval_factor(Mul, span, number.as_f64())),
        _ => None,
    }
}

fn date_difference(left: i32, right: i32) -> Result<Outcome, TypeInputError> {
    if [left, right]
        .iter()
        .any(|date| *date == DATE_NOBEGIN || *date == DATE_NOEND)
    {
        return Err(range_error("cannot subtract infinite dates"));
    }
    Ok(Outcome::Int(i64::from(left) - i64::from(right)))
}

/// Brings two temporal values to a common type for comparison.
fn common(
    left: &TypedValue,
    right: &TypedValue,
    session: &TemporalSession,
) -> Option<(TypedValue, TypedValue)> {
    use TypedValue::{Date, Timestamp as Ts, TimestampTz as Tstz};
    let as_timestamp = |date: i32| date_to_timestamp(date).ok();
    let zoned = |local: i64| {
        if is_infinite_timestamp(local) {
            local
        } else {
            session.zone.local_to_utc(local)
        }
    };
    Some(match (left, right) {
        (Date(date), Ts(_)) => (Ts(as_timestamp(*date)?), right.clone()),
        (Ts(_), Date(date)) => (left.clone(), Ts(as_timestamp(*date)?)),
        (Date(date), Tstz(_)) => (Tstz(zoned(as_timestamp(*date)?)), right.clone()),
        (Tstz(_), Date(date)) => (left.clone(), Tstz(zoned(as_timestamp(*date)?))),
        (Ts(local), Tstz(_)) => (Tstz(zoned(*local)), right.clone()),
        (Tstz(_), Ts(local)) => (left.clone(), Tstz(zoned(*local))),
        _ if left.kind() == right.kind() => (left.clone(), right.clone()),
        _ => return None,
    })
}

/// Compares two temporal values, converting across date/timestamp types.
#[must_use]
pub fn compare(
    left: &TypedValue,
    right: &TypedValue,
    session: &TemporalSession,
) -> Option<Ordering> {
    let (left, right) = common(left, right, session)?;
    Some(left.key().cmp(&right.key()))
}

/// Whether a value is one of the temporal carriers.
#[must_use]
pub const fn is_temporal(value: &TypedValue) -> bool {
    matches!(
        value,
        TypedValue::Date(_)
            | TypedValue::Time(_)
            | TypedValue::TimeTz(_)
            | TypedValue::Timestamp(_)
            | TypedValue::TimestampTz(_)
            | TypedValue::Interval(_)
    )
}

/// `isfinite`.
#[must_use]
pub fn is_finite(value: &TypedValue) -> bool {
    match value {
        TypedValue::Date(date) => *date != DATE_NOBEGIN && *date != DATE_NOEND,
        TypedValue::Timestamp(value) | TypedValue::TimestampTz(value) => {
            !is_infinite_timestamp(*value)
        }
        TypedValue::Interval(value) => !value.is_infinite(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{arithmetic, compare, ArithmeticOp, Number, Operand, Outcome};
    use crate::types::extended::temporal::{
        parse_date, parse_interval, parse_timestamp, parse_timestamptz, zone::lookup_zone,
        TemporalSession,
    };
    use crate::types::typed::TypedValue;

    fn session() -> TemporalSession {
        TemporalSession {
            zone: lookup_zone("America/New_York").expect("zone"),
            ..TemporalSession::default()
        }
    }

    fn typed(value: TypedValue) -> Operand {
        Operand::Typed(value)
    }

    #[test]
    fn should_add_calendar_months_and_days_like_postgresql() {
        // Arrange
        let session = session();
        let date = typed(TypedValue::Date(
            parse_date("2001-01-31", &session).expect("date"),
        ));
        let month = typed(TypedValue::Interval(
            parse_interval("1 month", -1, &session).expect("interval"),
        ));
        let day = typed(TypedValue::Interval(
            parse_interval("1 day", -1, &session).expect("interval"),
        ));
        let instant = typed(TypedValue::TimestampTz(
            parse_timestamptz("2024-03-09 12:00:00", &session, None).expect("instant"),
        ));

        // Act
        let clamped = arithmetic(&date, ArithmeticOp::Add, &month, &session);
        let across_dst = arithmetic(&instant, ArithmeticOp::Add, &day, &session);
        let shifted = arithmetic(
            &date,
            ArithmeticOp::Add,
            &Operand::Number(Number::Int(7)),
            &session,
        );

        // Assert
        assert_eq!(
            clamped,
            Some(Ok(Outcome::Typed(TypedValue::Timestamp(
                parse_timestamp("2001-02-28", &session, None).expect("timestamp")
            ))))
        );
        assert_eq!(
            across_dst,
            Some(Ok(Outcome::Typed(TypedValue::TimestampTz(
                parse_timestamptz("2024-03-10 16:00:00+00", &session, None).expect("instant")
            ))))
        );
        assert_eq!(
            shifted,
            Some(Ok(Outcome::Typed(TypedValue::Date(
                parse_date("2001-02-07", &session).expect("date")
            ))))
        );
    }

    #[test]
    fn should_compare_dates_with_timestamps() {
        // Arrange
        let session = session();
        let date = TypedValue::Date(parse_date("2001-03-01", &session).expect("date"));
        let timestamp = TypedValue::Timestamp(
            parse_timestamp("2001-03-01", &session, None).expect("timestamp"),
        );

        // Act
        let ordering = compare(&date, &timestamp, &session);

        // Assert
        assert_eq!(ordering, Some(std::cmp::Ordering::Equal));
    }
}
