//! PostgreSQL's proleptic Gregorian calendar arithmetic (`datetime.c`).
//!
//! Dates are Julian day numbers; timestamps are microseconds since
//! 2000-01-01 00:00:00. Year `0` is 1 BC, `-1` is 2 BC, and so on.

pub const USECS_PER_SEC: i64 = 1_000_000;
pub const USECS_PER_MINUTE: i64 = 60 * USECS_PER_SEC;
pub const USECS_PER_HOUR: i64 = 60 * USECS_PER_MINUTE;
pub const USECS_PER_DAY: i64 = 24 * USECS_PER_HOUR;
pub const SECS_PER_DAY: i64 = 86_400;
pub const DAYS_PER_MONTH: i32 = 30;
pub const MONTHS_PER_YEAR: i32 = 12;

/// Julian day of 2000-01-01, PostgreSQL's epoch.
pub const POSTGRES_EPOCH_JDATE: i32 = 2_451_545;
/// Julian day of 1970-01-01.
pub const UNIX_EPOCH_JDATE: i32 = 2_440_588;
/// Seconds between the Unix and PostgreSQL epochs.
pub const UNIX_TO_POSTGRES_SECS: i64 = 946_684_800;

pub const DATE_END_JULIAN: i64 = 2_147_483_494;
pub const TIMESTAMP_END_JULIAN: i64 = 109_203_528;
pub const MIN_TIMESTAMP: i64 = -211_813_488_000_000_000;
pub const END_TIMESTAMP: i64 = 9_223_371_331_200_000_000;

pub const DATE_NOBEGIN: i32 = i32::MIN;
pub const DATE_NOEND: i32 = i32::MAX;
pub const TIMESTAMP_NOBEGIN: i64 = i64::MIN;
pub const TIMESTAMP_NOEND: i64 = i64::MAX;

const DAY_TAB: [[i32; 12]; 2] = [
    [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
    [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31],
];

/// Broken-down calendar fields (`struct pg_tm` plus fractional seconds).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fields {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub micros: i64,
}

#[must_use]
pub const fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days in `month` (1-based) of `year`.
#[must_use]
pub fn days_in_month(year: i32, month: i32) -> i32 {
    let index = usize::try_from(month.clamp(1, 12) - 1).unwrap_or(0);
    DAY_TAB[usize::from(is_leap(year))][index]
}

/// `date2j`: the Julian day number of a calendar date.
#[must_use]
pub fn date2j(year: i32, month: i32, day: i32) -> i64 {
    let (mut year, mut month) = (i64::from(year), i64::from(month));
    if month > 2 {
        month += 1;
        year += 4800;
    } else {
        month += 13;
        year += 4799;
    }
    let century = year / 100;
    let mut julian = year * 365 - 32_167;
    julian += year / 4 - century + century / 4;
    julian += 7834 * month / 256 + i64::from(day);
    julian
}

/// `j2date`: calendar fields of a non-negative Julian day number.
#[must_use]
pub fn j2date(jd: i64) -> (i32, i32, i32) {
    let mut julian = jd.max(0) + 32_044;
    let mut quad = julian / 146_097;
    let extra = (julian - quad * 146_097) * 4 + 3;
    julian += 60 + quad * 3 + extra / 146_097;
    quad = julian / 1461;
    julian -= quad * 1461;
    let mut y = julian * 4 / 1461;
    julian = if y == 0 {
        (julian + 306) % 366
    } else {
        (julian + 305) % 365
    } + 123;
    y += quad * 4;
    let year = y - 4800;
    let quad = julian * 2141 / 65_536;
    let day = julian - 7834 * quad / 256;
    let month = (quad + 10) % 12 + 1;
    (narrow(year), narrow(month), narrow(day))
}

/// `j2day`: day of week, 0 = Sunday.
#[must_use]
pub fn j2day(jd: i64) -> i32 {
    narrow((jd + 1).rem_euclid(7))
}

fn narrow(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value < 0 { i32::MIN } else { i32::MAX })
}

/// `IS_VALID_JULIAN`: whether the fields are within the Julian-day range.
#[must_use]
pub const fn is_valid_julian(year: i32, month: i32, _day: i32) -> bool {
    (year > -4713 || (year == -4713 && month >= 11)) && year < 5_874_898
}

/// Whether a date value (days since 2000-01-01) is finite and in range.
#[must_use]
pub fn is_valid_date(date: i64) -> bool {
    let min = -i64::from(POSTGRES_EPOCH_JDATE);
    let end = DATE_END_JULIAN - i64::from(POSTGRES_EPOCH_JDATE);
    (min..end).contains(&date)
}

#[must_use]
pub const fn is_valid_timestamp(value: i64) -> bool {
    value >= MIN_TIMESTAMP && value < END_TIMESTAMP
}

#[must_use]
pub fn time_value(hour: i32, minute: i32, second: i32, micros: i64) -> i64 {
    ((i64::from(hour) * 60 + i64::from(minute)) * 60 + i64::from(second)) * USECS_PER_SEC + micros
}

/// `tm2timestamp`: fields (local or UTC) to a timestamp; `None` when out of
/// range. `zone_west` is PostgreSQL's seconds-west displacement.
#[must_use]
pub fn fields_to_timestamp(fields: &Fields, zone_west: Option<i32>) -> Option<i64> {
    if !is_valid_julian(fields.year, fields.month, fields.day) {
        return None;
    }
    let date = date2j(fields.year, fields.month, fields.day) - i64::from(POSTGRES_EPOCH_JDATE);
    let time = time_value(fields.hour, fields.minute, fields.second, fields.micros);
    let mut result = date.checked_mul(USECS_PER_DAY)?.checked_add(time)?;
    if let Some(west) = zone_west {
        result = result.checked_add(i64::from(west) * USECS_PER_SEC)?;
    }
    is_valid_timestamp(result).then_some(result)
}

/// `timestamp2tm` without a zone: the fields of a finite timestamp.
#[must_use]
pub fn timestamp_to_fields(value: i64) -> Option<Fields> {
    let mut date = value.div_euclid(USECS_PER_DAY);
    let time = value.rem_euclid(USECS_PER_DAY);
    date += i64::from(POSTGRES_EPOCH_JDATE);
    if date < 0 || date > i64::from(i32::MAX) {
        return None;
    }
    let (year, month, day) = j2date(date);
    let (hour, minute, second, micros) = time_fields(time);
    Some(Fields {
        year,
        month,
        day,
        hour,
        minute,
        second,
        micros,
    })
}

/// `dt2time`: hour, minute, second and microsecond of a time-of-day value.
#[must_use]
pub fn time_fields(time: i64) -> (i32, i32, i32, i64) {
    let hour = time / USECS_PER_HOUR;
    let rest = time - hour * USECS_PER_HOUR;
    let minute = rest / USECS_PER_MINUTE;
    let rest = rest - minute * USECS_PER_MINUTE;
    let second = rest / USECS_PER_SEC;
    let micros = rest - second * USECS_PER_SEC;
    (narrow(hour), narrow(minute), narrow(second), micros)
}

/// Fields of a date value (days since 2000-01-01).
#[must_use]
pub fn date_to_fields(date: i32) -> (i32, i32, i32) {
    j2date(i64::from(date) + i64::from(POSTGRES_EPOCH_JDATE))
}

/// `date2isoweek`.
#[must_use]
pub fn iso_week(year: i32, month: i32, day: i32) -> i32 {
    let target = date2j(year, month, day);
    let mut fourth = date2j(year, 1, 4);
    let mut monday_offset = i64::from(j2day(fourth - 1));
    if target < fourth - monday_offset {
        fourth = date2j(year - 1, 1, 4);
        monday_offset = i64::from(j2day(fourth - 1));
    }
    let mut result = (target - (fourth - monday_offset)) / 7 + 1;
    if result >= 52 {
        fourth = date2j(year + 1, 1, 4);
        monday_offset = i64::from(j2day(fourth - 1));
        if target >= fourth - monday_offset {
            result = (target - (fourth - monday_offset)) / 7 + 1;
        }
    }
    narrow(result)
}

/// `date2isoyear`.
#[must_use]
pub fn iso_year(year: i32, month: i32, day: i32) -> i32 {
    let mut year = year;
    let target = date2j(year, month, day);
    let mut fourth = date2j(year, 1, 4);
    let mut monday_offset = i64::from(j2day(fourth - 1));
    if target < fourth - monday_offset {
        fourth = date2j(year - 1, 1, 4);
        monday_offset = i64::from(j2day(fourth - 1));
        year -= 1;
    }
    let result = (target - (fourth - monday_offset)) / 7 + 1;
    if result >= 52 {
        fourth = date2j(year + 1, 1, 4);
        monday_offset = i64::from(j2day(fourth - 1));
        if target >= fourth - monday_offset {
            year += 1;
        }
    }
    year
}

/// `isoweek2j`: Julian day of the Monday of ISO `week` in ISO `year`.
#[must_use]
pub fn iso_week_to_julian(year: i32, week: i32) -> i64 {
    let fourth = date2j(year, 1, 4);
    let monday_offset = i64::from(j2day(fourth - 1));
    i64::from(week - 1) * 7 + (fourth - monday_offset)
}

/// `date2isoyearday`.
#[must_use]
pub fn iso_year_day(year: i32, month: i32, day: i32) -> i32 {
    narrow(date2j(year, month, day) - iso_week_to_julian(iso_year(year, month, day), 1) + 1)
}

const SCALES: [i64; 7] = [1_000_000, 100_000, 10_000, 1_000, 100, 10, 1];
const OFFSETS: [i64; 7] = [500_000, 50_000, 5_000, 500, 50, 5, 0];

/// `AdjustTimestampForTypmod`: rounds a finite timestamp half away from zero.
#[must_use]
pub fn round_timestamp(value: i64, precision: Option<u8>) -> i64 {
    let Some(precision) = precision
        .map(usize::from)
        .filter(|precision| *precision < 6)
    else {
        return value;
    };
    if value == TIMESTAMP_NOBEGIN || value == TIMESTAMP_NOEND {
        return value;
    }
    let (scale, offset) = (SCALES[precision], OFFSETS[precision]);
    if value >= 0 {
        (value.saturating_add(offset) / scale) * scale
    } else {
        -((value.saturating_neg().saturating_add(offset) / scale) * scale)
    }
}

/// `AdjustTimeForTypmod`.
#[must_use]
pub fn round_time(value: i64, precision: Option<u8>) -> i64 {
    let Some(precision) = precision
        .map(usize::from)
        .filter(|precision| *precision < 6)
    else {
        return value;
    };
    ((value + OFFSETS[precision]) / SCALES[precision]) * SCALES[precision]
}

/// Rounds an interval's time part the way `AdjustIntervalForTypmod` does.
#[must_use]
pub fn round_interval_time(value: i64, precision: usize) -> Option<i64> {
    if precision >= 6 {
        return Some(value);
    }
    let (scale, offset) = (SCALES[precision], OFFSETS[precision]);
    let shifted = if value >= 0 {
        value.checked_add(offset)?
    } else {
        value.checked_sub(offset)?
    };
    Some(shifted - shifted % scale)
}

#[cfg(test)]
mod tests {
    use super::{date2j, iso_week, iso_year, j2date, j2day, round_timestamp, POSTGRES_EPOCH_JDATE};

    #[test]
    fn should_round_trip_julian_days_across_eras() {
        // Arrange
        let dates = [
            (2000, 1, 1),
            (-4713, 11, 24),
            (0, 12, 31),
            (294_276, 12, 31),
        ];

        // Act
        let round_trips = dates.map(|(year, month, day)| j2date(date2j(year, month, day)));

        // Assert
        assert_eq!(round_trips, dates);
        assert_eq!(date2j(2000, 1, 1), i64::from(POSTGRES_EPOCH_JDATE));
        assert_eq!(j2day(date2j(2006, 1, 1)), 0);
    }

    #[test]
    fn should_number_iso_weeks_at_year_boundaries() {
        // Arrange
        let date = (2006, 1, 1);

        // Act
        let week = iso_week(date.0, date.1, date.2);
        let year = iso_year(date.0, date.1, date.2);

        // Assert
        assert_eq!((week, year), (52, 2005));
    }

    #[test]
    fn should_round_timestamps_half_away_from_zero() {
        // Arrange
        let values = [1_235_i64 * 100, -1_235 * 100];

        // Act
        let rounded = values.map(|value| round_timestamp(value, Some(3)));

        // Assert
        assert_eq!(rounded, [124_000, -124_000]);
    }
}
