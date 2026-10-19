//! `date_trunc`, `date_part`, `date_bin`, `age`, `make_*` and the other
//! date/time functions of `timestamp.c` and `date.c`.
use super::calendar::{
    date2j, days_in_month, fields_to_timestamp, iso_week, iso_week_to_julian, iso_year, j2date,
    j2day, time_fields, timestamp_to_fields, Fields, DAYS_PER_MONTH, MONTHS_PER_YEAR,
    POSTGRES_EPOCH_JDATE, TIMESTAMP_END_JULIAN, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND,
    UNIX_EPOCH_JDATE, UNIX_TO_POSTGRES_SECS, USECS_PER_DAY, USECS_PER_HOUR, USECS_PER_MINUTE,
    USECS_PER_SEC,
};
use super::tokens::{field_unit, round_half_even, Unit};
use super::zone::Zone;
use super::{range_error, Interval, TemporalSession};
use crate::types::extended::TypeInputError;

fn error(sqlstate: &'static str, message: impl Into<String>) -> TypeInputError {
    TypeInputError {
        sqlstate,
        message: message.into(),
    }
}

fn unit_of(name: &str, type_name: &str) -> Result<Unit, TypeInputError> {
    field_unit(name).ok_or_else(|| {
        error(
            "22023",
            format!(
                "unit \"{}\" not recognized for type {type_name}",
                name.to_ascii_lowercase()
            ),
        )
    })
}

fn unsupported(name: &str, type_name: &str) -> TypeInputError {
    error(
        "0A000",
        format!(
            "unit \"{}\" not supported for type {type_name}",
            name.to_ascii_lowercase()
        ),
    )
}

const fn is_infinite(value: i64) -> bool {
    value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND
}

/// Truncates calendar fields in place; returns whether the zone offset
/// must be recomputed (day or coarser).
fn truncate_fields(fields: &mut Fields, unit: Unit) -> Option<bool> {
    if unit == Unit::Week {
        let week = iso_week(fields.year, fields.month, fields.day);
        let mut year = fields.year;
        if week >= 52 && fields.month == 1 {
            year -= 1;
        }
        if week <= 1 && fields.month == MONTHS_PER_YEAR {
            year += 1;
        }
        let (y, m, d) = j2date(iso_week_to_julian(year, week));
        *fields = Fields {
            year: y,
            month: m,
            day: d,
            ..Fields::default()
        };
        return Some(true);
    }
    let order = [
        Unit::Millennium,
        Unit::Century,
        Unit::Decade,
        Unit::Year,
        Unit::Quarter,
        Unit::Month,
        Unit::Day,
        Unit::Hour,
        Unit::Minute,
        Unit::Second,
        Unit::Millisecond,
        Unit::Microsecond,
    ];
    let start = order.iter().position(|candidate| *candidate == unit)?;
    let year = fields.year;
    for step in &order[start..] {
        match step {
            Unit::Millennium => {
                fields.year = if year > 0 {
                    ((year + 999) / 1000) * 1000 - 999
                } else {
                    -((999 - (year - 1)) / 1000) * 1000 + 1
                };
            }
            Unit::Century if unit != Unit::Millennium => {
                fields.year = if year > 0 {
                    ((year + 99) / 100) * 100 - 99
                } else {
                    -((99 - (year - 1)) / 100) * 100 + 1
                };
            }
            Unit::Decade if unit == Unit::Decade => {
                fields.year = if year > 0 {
                    (year / 10) * 10
                } else {
                    -((8 - (year - 1)) / 10) * 10
                };
            }
            Unit::Year => fields.month = 1,
            Unit::Quarter => fields.month = 3 * ((fields.month - 1) / 3) + 1,
            Unit::Month => fields.day = 1,
            Unit::Day => fields.hour = 0,
            Unit::Hour => fields.minute = 0,
            Unit::Minute => fields.second = 0,
            Unit::Second => fields.micros = 0,
            Unit::Millisecond if unit == Unit::Millisecond => {
                fields.micros = (fields.micros / 1000) * 1000;
            }
            _ => {}
        }
    }
    Some(start <= 6)
}

/// `timestamp_trunc`.
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn trunc_timestamp(name: &str, value: i64) -> Result<i64, TypeInputError> {
    let unit = unit_of(name, "timestamp without time zone")?;
    if is_infinite(value) {
        return Ok(value);
    }
    let mut fields =
        timestamp_to_fields(value).ok_or_else(|| range_error("timestamp out of range"))?;
    truncate_fields(&mut fields, unit)
        .ok_or_else(|| unsupported(name, "timestamp without time zone"))?;
    fields_to_timestamp(&fields, None).ok_or_else(|| range_error("timestamp out of range"))
}

/// `timestamptz_trunc[_zone]`: truncates in `zone`'s local time.
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn trunc_timestamptz(name: &str, value: i64, zone: &Zone) -> Result<i64, TypeInputError> {
    let unit = unit_of(name, "timestamp with time zone")?;
    if is_infinite(value) {
        return Ok(value);
    }
    let east = zone.offset_at(value);
    let local = value + i64::from(east) * USECS_PER_SEC;
    let mut fields =
        timestamp_to_fields(local).ok_or_else(|| range_error("timestamp out of range"))?;
    let redo = truncate_fields(&mut fields, unit)
        .ok_or_else(|| unsupported(name, "timestamp with time zone"))?;
    let local =
        fields_to_timestamp(&fields, None).ok_or_else(|| range_error("timestamp out of range"))?;
    Ok(if redo {
        zone.local_to_utc(local)
    } else {
        local - i64::from(east) * USECS_PER_SEC
    })
}

/// `interval_trunc`.
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn trunc_interval(name: &str, value: &Interval) -> Result<Interval, TypeInputError> {
    let unit = unit_of(name, "interval")?;
    if value.is_infinite() {
        return Ok(*value);
    }
    let (mut year, mut month) = (
        value.months / MONTHS_PER_YEAR,
        value.months % MONTHS_PER_YEAR,
    );
    let mut day = value.days;
    let mut time = value.micros;
    let hour = time / USECS_PER_HOUR;
    let minute = (time % USECS_PER_HOUR) / USECS_PER_MINUTE;
    let second = (time % USECS_PER_MINUTE) / USECS_PER_SEC;
    let micros = time % USECS_PER_SEC;
    let rebuild = |hour: i64, minute: i64, second: i64, micros: i64| {
        hour * USECS_PER_HOUR + minute * USECS_PER_MINUTE + second * USECS_PER_SEC + micros
    };
    match unit {
        Unit::Millennium | Unit::Century | Unit::Decade | Unit::Year | Unit::Quarter => {
            year = match unit {
                Unit::Millennium => (year / 1000) * 1000,
                Unit::Century => (year / 100) * 100,
                Unit::Decade => (year / 10) * 10,
                _ => year,
            };
            month = if unit == Unit::Quarter {
                3 * (month / 3)
            } else {
                0
            };
            day = 0;
            time = 0;
        }
        Unit::Month => {
            day = 0;
            time = 0;
        }
        Unit::Day => time = 0,
        Unit::Hour => time = rebuild(hour, 0, 0, 0),
        Unit::Minute => time = rebuild(hour, minute, 0, 0),
        Unit::Second => time = rebuild(hour, minute, second, 0),
        Unit::Millisecond => time = rebuild(hour, minute, second, (micros / 1000) * 1000),
        Unit::Microsecond => {}
        _ => return Err(unsupported(name, "interval")),
    }
    Ok(Interval::new(year * MONTHS_PER_YEAR + month, day, time))
}

fn decade(year: i32) -> i32 {
    if year >= 0 {
        year / 10
    } else {
        -((8 - (year - 1)) / 10)
    }
}

fn century(year: i32) -> i32 {
    if year > 0 {
        (year + 99) / 100
    } else {
        -((99 - (year - 1)) / 100)
    }
}

fn millennium(year: i32) -> i32 {
    if year > 0 {
        (year + 999) / 1000
    } else {
        -((999 - (year - 1)) / 1000)
    }
}

fn f(value: i64) -> f64 {
    num_traits::ToPrimitive::to_f64(&value).unwrap_or(0.0)
}

/// `timestamp_part`/`timestamptz_part`: `zone_east` is the displacement of
/// the local fields (for `timezone*` units and the epoch of a local time).
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn part_timestamp(
    name: &str,
    local: i64,
    zone_east: Option<i32>,
    type_name: &str,
) -> Result<Option<f64>, TypeInputError> {
    let unit = unit_of(name, type_name)?;
    if is_infinite(local) {
        let sign = if local == TIMESTAMP_NOEND { 1.0 } else { -1.0 };
        return Ok(match unit {
            Unit::Year
            | Unit::Decade
            | Unit::Century
            | Unit::Millennium
            | Unit::Julian
            | Unit::IsoYear
            | Unit::Epoch => Some(sign * f64::INFINITY),
            _ => None,
        });
    }
    let fields = timestamp_to_fields(local).ok_or_else(|| range_error("timestamp out of range"))?;
    let seconds = f(i64::from(fields.second)) + f(fields.micros) / 1_000_000.0;
    let year = fields.year;
    let value = match unit {
        Unit::Microsecond => f(i64::from(fields.second) * USECS_PER_SEC + fields.micros),
        Unit::Millisecond => f(i64::from(fields.second)) * 1000.0 + f(fields.micros) / 1000.0,
        Unit::Second => seconds,
        Unit::Minute => f64::from(fields.minute),
        Unit::Hour => f64::from(fields.hour),
        Unit::Day => f64::from(fields.day),
        Unit::Month => f64::from(fields.month),
        Unit::Quarter => f64::from((fields.month - 1) / 3 + 1),
        Unit::Week => f64::from(iso_week(year, fields.month, fields.day)),
        Unit::Year => f64::from(if year > 0 { year } else { year - 1 }),
        Unit::Decade => f64::from(decade(year)),
        Unit::Century => f64::from(century(year)),
        Unit::Millennium => f64::from(millennium(year)),
        Unit::Julian => {
            f(date2j(year, fields.month, fields.day))
                + (f64::from(fields.hour) * 3600.0 + f64::from(fields.minute) * 60.0 + seconds)
                    / 86_400.0
        }
        Unit::IsoYear => {
            let iso = iso_year(year, fields.month, fields.day);
            f64::from(if iso > 0 { iso } else { iso - 1 })
        }
        Unit::Dow => f64::from(j2day(date2j(year, fields.month, fields.day))),
        Unit::IsoDow => {
            let day = j2day(date2j(year, fields.month, fields.day));
            f64::from(if day == 0 { 7 } else { day })
        }
        Unit::Doy => f(date2j(year, fields.month, fields.day) - date2j(year, 1, 1) + 1),
        Unit::Epoch => {
            let utc = local - i64::from(zone_east.unwrap_or(0)) * USECS_PER_SEC;
            f(utc - (-UNIX_TO_POSTGRES_SECS * USECS_PER_SEC)) / 1_000_000.0
        }
        Unit::Timezone | Unit::TimezoneHour | Unit::TimezoneMinute => {
            let east = zone_east.ok_or_else(|| unsupported(name, type_name))?;
            zone_part(unit, east)
        }
    };
    Ok(Some(value))
}

fn zone_part(unit: Unit, east: i32) -> f64 {
    match unit {
        Unit::TimezoneHour => f64::from(east / 3600),
        Unit::TimezoneMinute => f64::from((east / 60) % 60),
        _ => f64::from(east),
    }
}

/// `time_part`/`timetz_part`.
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn part_time(name: &str, time: i64, zone_east: Option<i32>) -> Result<f64, TypeInputError> {
    let type_name = if zone_east.is_some() {
        "time with time zone"
    } else {
        "time without time zone"
    };
    let unit = unit_of(name, type_name)?;
    let (hour, minute, second, micros) = time_fields(time);
    Ok(match unit {
        Unit::Microsecond => f(i64::from(second) * USECS_PER_SEC + micros),
        Unit::Millisecond => f64::from(second) * 1000.0 + f(micros) / 1000.0,
        Unit::Second => f64::from(second) + f(micros) / 1_000_000.0,
        Unit::Minute => f64::from(minute),
        Unit::Hour => f64::from(hour),
        Unit::Epoch => f(time - i64::from(zone_east.unwrap_or(0)) * USECS_PER_SEC) / 1_000_000.0,
        Unit::Timezone | Unit::TimezoneHour | Unit::TimezoneMinute => {
            zone_part(unit, zone_east.ok_or_else(|| unsupported(name, type_name))?)
        }
        _ => return Err(unsupported(name, type_name)),
    })
}

/// `interval_part`.
///
/// # Errors
/// Returns PostgreSQL's unit errors.
pub fn part_interval(name: &str, value: &Interval) -> Result<Option<f64>, TypeInputError> {
    let unit = unit_of(name, "interval")?;
    if value.is_infinite() {
        let sign = if *value == Interval::INFINITY {
            1.0
        } else {
            -1.0
        };
        return Ok(match unit {
            Unit::Year
            | Unit::Decade
            | Unit::Century
            | Unit::Millennium
            | Unit::Epoch
            | Unit::Hour
            | Unit::Day => Some(sign * f64::INFINITY),
            _ => None,
        });
    }
    let year = value.months / MONTHS_PER_YEAR;
    let month = value.months % MONTHS_PER_YEAR;
    let hour = value.micros / USECS_PER_HOUR;
    let minute = (value.micros % USECS_PER_HOUR) / USECS_PER_MINUTE;
    let second_micros = value.micros % USECS_PER_MINUTE;
    Ok(Some(match unit {
        Unit::Microsecond => f(second_micros),
        Unit::Millisecond => f(second_micros) / 1000.0,
        Unit::Second => f(second_micros) / 1_000_000.0,
        Unit::Minute => f(minute),
        Unit::Hour => f(hour),
        Unit::Day => f64::from(value.days),
        Unit::Month => f64::from(month),
        Unit::Quarter => f64::from(month / 3 + 1),
        Unit::Year => f64::from(year),
        Unit::Decade => f64::from(year / 10),
        Unit::Century => f64::from(year / 100),
        Unit::Millennium => f64::from(year / 1000),
        Unit::Epoch => {
            f(value.micros) / 1_000_000.0
                + 365.25 * 86_400.0 * f64::from(value.months / MONTHS_PER_YEAR)
                + f64::from(DAYS_PER_MONTH) * 86_400.0 * f64::from(value.months % MONTHS_PER_YEAR)
                + 86_400.0 * f64::from(value.days)
        }
        _ => return Err(unsupported(name, "interval")),
    }))
}

/// `date_bin` for `timestamp` and `timestamptz`.
///
/// # Errors
/// Returns PostgreSQL's stride and range errors.
pub fn date_bin(stride: &Interval, source: i64, origin: i64) -> Result<i64, TypeInputError> {
    if is_infinite(source) {
        return Ok(source);
    }
    if is_infinite(origin) {
        return Err(range_error("origin out of range"));
    }
    if stride.is_infinite() {
        return Err(error(
            "0A000",
            "timestamps cannot be binned into infinite intervals",
        ));
    }
    if stride.months != 0 {
        return Err(error(
            "0A000",
            "timestamps cannot be binned into intervals containing months or years",
        ));
    }
    let stride_micros = i64::from(stride.days)
        .checked_mul(USECS_PER_DAY)
        .and_then(|micros| micros.checked_add(stride.micros))
        .ok_or_else(|| range_error("interval out of range"))?;
    if stride_micros <= 0 {
        return Err(range_error("stride must be greater than zero"));
    }
    let difference = source
        .checked_sub(origin)
        .ok_or_else(|| range_error("interval out of range"))?;
    let modulo = difference % stride_micros;
    let mut result = origin + (difference - modulo);
    if modulo < 0 {
        result = result
            .checked_sub(stride_micros)
            .filter(|value| super::calendar::is_valid_timestamp(*value))
            .ok_or_else(|| range_error("timestamp out of range"))?;
    }
    Ok(result)
}

/// `timestamp_age` over local fields (`timestamptz_age` passes local times).
///
/// # Errors
/// Returns `22008` for out-of-range results.
pub fn age(left: i64, right: i64) -> Result<Interval, TypeInputError> {
    if is_infinite(left) || is_infinite(right) {
        return match (left, right) {
            (TIMESTAMP_NOEND, TIMESTAMP_NOEND) | (TIMESTAMP_NOBEGIN, TIMESTAMP_NOBEGIN) => {
                Err(range_error("interval out of range"))
            }
            (TIMESTAMP_NOEND, _) | (_, TIMESTAMP_NOBEGIN) => Ok(Interval::INFINITY),
            _ => Ok(Interval::NEG_INFINITY),
        };
    }
    let (Some(first), Some(second)) = (timestamp_to_fields(left), timestamp_to_fields(right))
    else {
        return Err(range_error("timestamp out of range"));
    };
    let negate = left < right;
    let sign = if negate { -1 } else { 1 };
    let mut micros = (first.micros - second.micros) * i64::from(sign);
    let mut second_part = (first.second - second.second) * sign;
    let mut minute = (first.minute - second.minute) * sign;
    let mut hour = (first.hour - second.hour) * sign;
    let mut day = (first.day - second.day) * sign;
    let mut month = (first.month - second.month) * sign;
    let mut year = (first.year - second.year) * sign;
    while micros < 0 {
        micros += USECS_PER_SEC;
        second_part -= 1;
    }
    while second_part < 0 {
        second_part += 60;
        minute -= 1;
    }
    while minute < 0 {
        minute += 60;
        hour -= 1;
    }
    while hour < 0 {
        hour += 24;
        day -= 1;
    }
    while day < 0 {
        let base = if negate { first } else { second };
        day += days_in_month(base.year, base.month);
        month -= 1;
    }
    while month < 0 {
        month += MONTHS_PER_YEAR;
        year -= 1;
    }
    let months = year
        .checked_mul(MONTHS_PER_YEAR)
        .and_then(|value| value.checked_add(month))
        .ok_or_else(|| range_error("interval out of range"))?;
    let time = i64::from(hour) * USECS_PER_HOUR
        + i64::from(minute) * USECS_PER_MINUTE
        + i64::from(second_part) * USECS_PER_SEC
        + micros;
    Ok(Interval::new(
        months * sign,
        day * sign,
        time * i64::from(sign),
    ))
}

fn validated_year(year: i32) -> Option<(i32, bool)> {
    match year {
        0 => None,
        year if year < 0 => Some((-(year) - 1, true)),
        year => Some((year, false)),
    }
}

/// `make_date`.
///
/// # Errors
/// Returns `22008` for invalid fields.
pub fn make_date(year: i32, month: i32, day: i32) -> Result<i32, TypeInputError> {
    let invalid = || {
        range_error(format!(
            "date field value out of range: {year}-{month:02}-{day:02}"
        ))
    };
    let (internal, _) = validated_year(year).ok_or_else(invalid)?;
    let internal = if year < 0 { -internal } else { internal };
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(internal, month) {
        return Err(invalid());
    }
    if !super::calendar::is_valid_julian(internal, month, day) {
        return Err(range_error(format!(
            "date out of range: {year}-{month:02}-{day:02}"
        )));
    }
    let date = date2j(internal, month, day) - i64::from(POSTGRES_EPOCH_JDATE);
    if !super::calendar::is_valid_date(date) {
        return Err(range_error(format!(
            "date out of range: {year}-{month:02}-{day:02}"
        )));
    }
    i32::try_from(date).map_err(|_| invalid())
}

/// `make_time` with fractional seconds.
///
/// # Errors
/// Returns `22008` for invalid fields.
pub fn make_time(hour: i32, minute: i32, seconds: f64) -> Result<i64, TypeInputError> {
    let invalid = || {
        range_error(format!(
            "time field value out of range: {hour}:{minute:02}:{}",
            format_seconds(seconds)
        ))
    };
    if !(0..=24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0.0..=60.0).contains(&seconds)
        || seconds.is_nan()
    {
        return Err(invalid());
    }
    let time = i64::from(hour) * USECS_PER_HOUR
        + i64::from(minute) * USECS_PER_MINUTE
        + round_half_even(seconds * 1_000_000.0);
    if time > USECS_PER_DAY {
        return Err(invalid());
    }
    Ok(time)
}

fn format_seconds(seconds: f64) -> String {
    if seconds.fract() == 0.0 {
        format!("{seconds:02}")
    } else {
        format!("{seconds}")
    }
}

/// `make_timestamp`: local fields as a `timestamp`.
///
/// # Errors
/// Returns `22008` for invalid fields.
pub fn make_timestamp(
    fields: (i32, i32, i32, i32, i32),
    seconds: f64,
) -> Result<i64, TypeInputError> {
    let (year, month, day, hour, minute) = fields;
    let date = make_date(year, month, day)?;
    let time = make_time(hour, minute, seconds)?;
    let value = i64::from(date) * USECS_PER_DAY + time;
    if !super::calendar::is_valid_timestamp(value) {
        return Err(range_error(format!(
            "timestamp out of range: {year}-{month:02}-{day:02} {hour}:{minute:02}:{}",
            format_seconds(seconds)
        )));
    }
    Ok(value)
}

/// `make_interval`.
///
/// # Errors
/// Returns `22008` when a component overflows.
pub fn make_interval(parts: [i32; 6], seconds: f64) -> Result<Interval, TypeInputError> {
    let overflow = || range_error("interval out of range");
    let [years, months, weeks, days, hours, minutes] = parts;
    if seconds.is_nan() || seconds.is_infinite() {
        return Err(overflow());
    }
    let months = years
        .checked_mul(MONTHS_PER_YEAR)
        .and_then(|value| value.checked_add(months))
        .ok_or_else(overflow)?;
    let days = weeks
        .checked_mul(7)
        .and_then(|value| value.checked_add(days))
        .ok_or_else(overflow)?;
    let secs = round_half_even(seconds * 1_000_000.0);
    let micros = i64::from(hours)
        .checked_mul(USECS_PER_HOUR)
        .and_then(|value| value.checked_add(i64::from(minutes) * USECS_PER_MINUTE))
        .and_then(|value| value.checked_add(secs))
        .ok_or_else(overflow)?;
    let interval = Interval::new(months, days, micros);
    if interval.is_infinite() {
        return Err(overflow());
    }
    Ok(interval)
}

/// `to_timestamp(double precision)`.
///
/// # Errors
/// Returns `22008` for NaN or out-of-range epochs.
pub fn to_timestamp_epoch(seconds: f64) -> Result<i64, TypeInputError> {
    if seconds.is_nan() {
        return Err(range_error("timestamp cannot be NaN"));
    }
    if seconds.is_infinite() {
        return Ok(if seconds > 0.0 {
            TIMESTAMP_NOEND
        } else {
            TIMESTAMP_NOBEGIN
        });
    }
    let lower = 86_400.0 * f(-i64::from(UNIX_EPOCH_JDATE));
    let upper = 86_400.0 * f(TIMESTAMP_END_JULIAN - i64::from(UNIX_EPOCH_JDATE));
    if seconds < lower || seconds >= upper {
        return Err(range_error(format!(
            "timestamp out of range: \"{seconds}\""
        )));
    }
    let shifted = seconds - f(UNIX_TO_POSTGRES_SECS);
    Ok(round_half_even(shifted * 1_000_000.0))
}

/// The local timestamp of an instant in a zone.
#[must_use]
pub fn local_of(value: i64, zone: &Zone) -> i64 {
    if is_infinite(value) {
        value
    } else {
        zone.utc_to_local(value)
    }
}

/// The instant of a local time in a zone.
#[must_use]
pub fn utc_of(value: i64, zone: &Zone) -> i64 {
    if is_infinite(value) {
        value
    } else {
        zone.local_to_utc(value)
    }
}

/// `DecodeTimezoneName`: an abbreviation (session zone first), a zone name,
/// or a POSIX rule.
///
/// # Errors
/// Returns `22023` for an unknown zone.
pub fn zone_named(name: &str, session: &TemporalSession) -> Result<Zone, TypeInputError> {
    let lower = name.to_ascii_lowercase();
    let abbreviation = lower
        .chars()
        .all(|c| c.is_ascii_alphabetic())
        .then(|| {
            session
                .zone
                .abbreviation_offset(&lower, session.transaction_start)
                .or_else(|| session.abbreviations.lookup(&lower))
        })
        .flatten();
    if let Some((east, _)) = abbreviation {
        return Ok(Zone::Fixed {
            east,
            name: name.to_string(),
        });
    }
    super::zone::lookup_zone(name)
        .ok_or_else(|| error("22023", format!("time zone \"{name}\" not recognized")))
}

/// A zone from an interval displacement (`AT TIME ZONE INTERVAL '...'`).
///
/// # Errors
/// Returns `22023` when the interval has months or days.
pub fn zone_interval(value: &Interval) -> Result<Zone, TypeInputError> {
    if value.months != 0 || value.days != 0 {
        return Err(error(
            "22023",
            format!(
                "interval time zone \"{}\" must not include months or days",
                value.format(super::session::IntervalStyle::Postgres)
            ),
        ));
    }
    let east = i32::try_from(value.micros / USECS_PER_SEC)
        .map_err(|_| error("22023", "interval time zone out of range"))?;
    Ok(super::zone::fixed_zone(east))
}

#[cfg(test)]
mod tests {
    use super::{age, date_bin, part_timestamp, trunc_interval, trunc_timestamp};
    use crate::types::extended::temporal::{parse_interval, parse_timestamp, TemporalSession};

    fn ts(text: &str) -> i64 {
        parse_timestamp(text, &TemporalSession::default(), None).expect("timestamp")
    }

    #[test]
    fn should_truncate_and_extract_like_postgresql() {
        // Arrange
        let session = TemporalSession::default();
        let interval = parse_interval("2 days 3 hours 40 minutes", -1, &session).expect("interval");

        // Act
        let quarter = trunc_timestamp("quarter", ts("2001-05-16 20:38:40"));
        let week = trunc_timestamp("week", ts("2001-02-16 20:38:40"));
        let hours = trunc_interval("hour", &interval);
        let iso_year = part_timestamp("isoyear", ts("2006-01-01"), None, "timestamp");

        // Assert
        assert_eq!(quarter.ok(), Some(ts("2001-04-01")));
        assert_eq!(week.ok(), Some(ts("2001-02-12")));
        assert_eq!(
            hours.ok(),
            parse_interval("2 days 3 hours", -1, &session).ok()
        );
        assert_eq!(iso_year.ok(), Some(Some(2005.0)));
    }

    #[test]
    fn should_compute_symbolic_age_and_bins() {
        // Arrange
        let session = TemporalSession::default();
        let stride = parse_interval("15 minutes", -1, &session).expect("stride");

        // Act
        let symbolic = age(ts("2004-06-01"), ts("2004-04-30"));
        let reversed = age(ts("2004-04-30"), ts("2004-06-01"));
        let bin = date_bin(
            &stride,
            ts("2020-02-11 15:44:17"),
            ts("2001-01-01 00:02:30"),
        );

        // Assert
        assert_eq!(
            symbolic.ok(),
            parse_interval("1 month 1 day", -1, &session).ok()
        );
        assert_eq!(
            reversed.ok(),
            parse_interval("-1 month -1 day", -1, &session).ok()
        );
        assert_eq!(bin.ok(), Some(ts("2020-02-11 15:32:30")));
    }
}
