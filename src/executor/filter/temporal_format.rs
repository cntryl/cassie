//! `to_char`, `to_date` and `to_timestamp(text, text)` over argument values.
use super::temporal::type_error;
use super::QueryError;
use crate::types::extended::temporal::calendar::{
    date2j, is_valid_timestamp, round_timestamp, time_value, timestamp_to_fields, Fields,
    POSTGRES_EPOCH_JDATE, USECS_PER_DAY, USECS_PER_HOUR, USECS_PER_MINUTE, USECS_PER_SEC,
};
use crate::types::extended::temporal::format::{self, Moment};
use crate::types::extended::temporal::ops;
use crate::types::extended::temporal::{range_error, TemporalSession};
use crate::types::typed::TypedValue;
use crate::types::Value;

#[path = "temporal_interval_template.rs"]
mod interval_template;

fn text(value: &Value) -> Result<String, QueryError> {
    match value {
        Value::String(text) => Ok(text.clone()),
        Value::Typed(typed) => Ok(typed.text()),
        _ => Err(QueryError::Cassie(crate::app::CassieError::Execution(
            "formatting functions require text arguments".into(),
        ))),
    }
}

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: &TemporalSession,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let template = text(&args[1])?;
    match name {
        "to_char" => to_char(&args[0], &template, session, controls),
        "to_date" => {
            let (parsed, _, _) =
                format::parse_with_precision(&text(&args[0])?, &template, session.abbreviations)
                    .map_err(type_error)?;
            let fields = parsed.fields;
            let date =
                date2j(fields.year, fields.month, fields.day) - i64::from(POSTGRES_EPOCH_JDATE);
            i32::try_from(date)
                .ok()
                .filter(|date| {
                    crate::types::extended::temporal::calendar::is_valid_date(i64::from(*date))
                })
                .map(|date| Value::Typed(TypedValue::Date(date)))
                .ok_or_else(|| type_error(range_error("date out of range")))
        }
        _ => {
            let (parsed, precision, abbreviation) =
                format::parse_with_precision(&text(&args[0])?, &template, session.abbreviations)
                    .map_err(type_error)?;
            // The local wall time may be just outside the timestamp range
            // while its UTC instant is valid after applying the displacement.
            let fields = parsed.fields;
            let date =
                date2j(fields.year, fields.month, fields.day) - i64::from(POSTGRES_EPOCH_JDATE);
            let local = date
                .checked_mul(USECS_PER_DAY)
                .and_then(|date| {
                    date.checked_add(time_value(
                        fields.hour,
                        fields.minute,
                        fields.second,
                        fields.micros,
                    ))
                })
                .ok_or_else(|| type_error(range_error("timestamp out of range")))?;
            let east = if let Some(abbreviation) = abbreviation {
                match abbreviation.rule {
                    format::ZoneRule::Fixed(east) => east,
                    format::ZoneRule::Dynamic(name) => {
                        let zone = crate::types::extended::temporal::zone::lookup_zone(name)
                            .ok_or_else(|| {
                                type_error(crate::types::extended::TypeInputError {
                                    sqlstate: "22007",
                                    message: format!("unavailable time zone: {name}"),
                                })
                            })?;
                        let offset = zone.offset_for_local(local);
                        let probe = local.saturating_sub(i64::from(offset) * USECS_PER_SEC);
                        let mut check = || -> Result<(), QueryError> {
                            if let Some(controls) = controls {
                                controls.check_interruption()?;
                            }
                            Ok(())
                        };
                        zone.abbreviation_offset_with_checks(abbreviation.name, probe, &mut check)?
                            .map_or(offset, |(east, _)| east)
                    }
                }
            } else {
                parsed
                    .zone_east
                    .unwrap_or_else(|| session.zone.offset_for_local(local))
            };
            let instant = local
                .checked_sub(i64::from(east) * USECS_PER_SEC)
                .filter(|value| is_valid_timestamp(*value))
                .map(|value| round_timestamp(value, precision))
                .filter(|value| is_valid_timestamp(*value))
                .ok_or_else(|| type_error(range_error("timestamp out of range")))?;
            Ok(Value::Typed(TypedValue::TimestampTz(instant)))
        }
    }
}

fn to_char(
    value: &Value,
    template: &str,
    session: &TemporalSession,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let Value::Typed(value) = value else {
        return Ok(Value::Null);
    };
    if !ops::is_finite(value) {
        return Ok(Value::Null);
    }
    if let TypedValue::Date(date) = value {
        let midnight = ops::date_to_timestamp(*date).map_err(type_error)?;
        let offset = i64::from(session.zone.offset_for_local(midnight)) * USECS_PER_SEC;
        let instant = midnight
            .checked_sub(offset)
            .filter(|instant| is_valid_timestamp(*instant))
            .ok_or_else(|| type_error(range_error("date out of range for timestamp")))?;
        if template.is_empty() {
            return Ok(Value::Null);
        }
        let info = session.zone.info_at(instant);
        let local = instant
            .checked_add(i64::from(info.east) * USECS_PER_SEC)
            .ok_or_else(|| type_error(range_error("timestamp out of range")))?;
        let moment = Moment {
            fields: timestamp_to_fields(local)
                .ok_or_else(|| type_error(range_error("timestamp out of range")))?,
            zone_east: Some(info.east),
            is_interval: false,
        };
        return Ok(Value::String(format::to_char(
            &moment,
            template,
            &info.abbreviation,
        )));
    }
    if template.is_empty() {
        return Ok(Value::Null);
    }
    if matches!(value, TypedValue::Interval(_))
        && interval_template::invalid(template, || {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            Ok::<(), QueryError>(())
        })?
    {
        let diagnostic = crate::app::DiagnosticError::new(
            "22007",
            "invalid format specification for an interval value",
        )
        .with_hint("Intervals are not tied to specific calendar dates.");
        return Err(QueryError::Cassie(diagnostic.into()));
    }
    let out_of_range = || type_error(range_error("timestamp out of range"));
    let (moment, zone_name) = match value {
        TypedValue::Timestamp(local) => (
            Moment {
                fields: timestamp_to_fields(*local).ok_or_else(out_of_range)?,
                zone_east: None,
                is_interval: false,
            },
            String::new(),
        ),
        TypedValue::TimestampTz(instant) => {
            let info = session.zone.info_at(*instant);
            let local = instant + i64::from(info.east) * USECS_PER_SEC;
            (
                Moment {
                    fields: timestamp_to_fields(local).ok_or_else(out_of_range)?,
                    zone_east: Some(info.east),
                    is_interval: false,
                },
                info.abbreviation,
            )
        }
        TypedValue::Interval(interval) => {
            let time = interval.micros;
            let fields = Fields {
                year: interval.months / 12,
                month: interval.months % 12,
                day: interval.days,
                hour: i32::try_from(time / USECS_PER_HOUR).unwrap_or(i32::MAX),
                minute: i32::try_from((time % USECS_PER_HOUR) / USECS_PER_MINUTE).unwrap_or(0),
                second: i32::try_from((time % USECS_PER_MINUTE) / USECS_PER_SEC).unwrap_or(0),
                micros: time % USECS_PER_SEC,
            };
            (
                Moment {
                    fields,
                    zone_east: None,
                    is_interval: true,
                },
                String::new(),
            )
        }
        _ => return Ok(Value::Null),
    };
    let hour = match value {
        TypedValue::Interval(interval) => interval.micros / USECS_PER_HOUR,
        _ => i64::from(moment.fields.hour),
    };
    Ok(Value::String(format::to_char_with_hour(
        &moment, template, &zone_name, hour,
    )))
}
