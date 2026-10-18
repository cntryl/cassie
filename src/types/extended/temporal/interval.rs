//! `interval`: months, days and microseconds, kept separately as PostgreSQL
//! does, with its output styles, normalized ordering and arithmetic.
use super::calendar::{
    round_interval_time, DAYS_PER_MONTH, MONTHS_PER_YEAR, SECS_PER_DAY, USECS_PER_DAY,
    USECS_PER_HOUR, USECS_PER_MINUTE, USECS_PER_SEC,
};
use super::session::IntervalStyle;
use super::tokens::round_half_even;
use std::fmt::Write as _;

#[path = "interval_input.rs"]
mod input;
pub use input::{interval_range, interval_typmod, parse_interval};

pub const MONTH_MASK: i32 = 1 << 1;
pub const YEAR_MASK: i32 = 1 << 2;
pub const DAY_MASK: i32 = 1 << 3;
pub const HOUR_MASK: i32 = 1 << 10;
pub const MINUTE_MASK: i32 = 1 << 11;
pub const SECOND_MASK: i32 = 1 << 12;
pub const FULL_RANGE: i32 = 0x7FFF;
pub const FULL_PRECISION: i32 = 0xFFFF;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Interval {
    pub months: i32,
    pub days: i32,
    pub micros: i64,
}

/// An interval arithmetic or range failure (`22008 interval out of range`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntervalOverflow;

impl Interval {
    pub const INFINITY: Self = Self {
        months: i32::MAX,
        days: i32::MAX,
        micros: i64::MAX,
    };
    pub const NEG_INFINITY: Self = Self {
        months: i32::MIN,
        days: i32::MIN,
        micros: i64::MIN,
    };

    #[must_use]
    pub const fn new(months: i32, days: i32, micros: i64) -> Self {
        Self {
            months,
            days,
            micros,
        }
    }

    #[must_use]
    pub fn is_infinite(&self) -> bool {
        *self == Self::INFINITY || *self == Self::NEG_INFINITY
    }

    /// `interval_cmp_value`: the 30-day-month, 24-hour-day ordering key.
    #[must_use]
    pub fn order_key(&self) -> i128 {
        i128::from(self.micros)
            + i128::from(self.days) * i128::from(USECS_PER_DAY)
            + i128::from(self.months) * i128::from(DAYS_PER_MONTH) * i128::from(USECS_PER_DAY)
    }

    /// `interval_um`.
    ///
    /// # Errors
    /// Returns an overflow for the most negative components.
    pub fn negate(&self) -> Result<Self, IntervalOverflow> {
        if *self == Self::INFINITY {
            return Ok(Self::NEG_INFINITY);
        }
        if *self == Self::NEG_INFINITY {
            return Ok(Self::INFINITY);
        }
        let result = Self {
            months: self.months.checked_neg().ok_or(IntervalOverflow)?,
            days: self.days.checked_neg().ok_or(IntervalOverflow)?,
            micros: self.micros.checked_neg().ok_or(IntervalOverflow)?,
        };
        finite(result)
    }

    /// `interval_pl`.
    ///
    /// # Errors
    /// Returns an overflow, or for infinities of opposite sign.
    pub fn add(&self, other: &Self) -> Result<Self, IntervalOverflow> {
        match (self.is_infinite(), other.is_infinite()) {
            (true, true) if self != other => return Err(IntervalOverflow),
            (true, _) => return Ok(*self),
            (_, true) => return Ok(*other),
            _ => {}
        }
        finite(Self {
            months: self
                .months
                .checked_add(other.months)
                .ok_or(IntervalOverflow)?,
            days: self.days.checked_add(other.days).ok_or(IntervalOverflow)?,
            micros: self
                .micros
                .checked_add(other.micros)
                .ok_or(IntervalOverflow)?,
        })
    }

    /// `interval_mi`.
    ///
    /// # Errors
    /// Returns an overflow, or for infinities of equal sign.
    pub fn sub(&self, other: &Self) -> Result<Self, IntervalOverflow> {
        if self.is_infinite() && other.is_infinite() {
            return if self == other {
                Err(IntervalOverflow)
            } else {
                Ok(*self)
            };
        }
        if other.is_infinite() {
            return other.negate();
        }
        if self.is_infinite() {
            return Ok(*self);
        }
        finite(Self {
            months: self
                .months
                .checked_sub(other.months)
                .ok_or(IntervalOverflow)?,
            days: self.days.checked_sub(other.days).ok_or(IntervalOverflow)?,
            micros: self
                .micros
                .checked_sub(other.micros)
                .ok_or(IntervalOverflow)?,
        })
    }

    /// `interval_mul`.
    ///
    /// # Errors
    /// Returns an overflow for out-of-range or undefined products.
    pub fn mul(&self, factor: f64) -> Result<Self, IntervalOverflow> {
        if factor.is_nan() {
            return Err(IntervalOverflow);
        }
        if self.is_infinite() || factor.is_infinite() {
            let sign = self.sign()
                * if factor > 0.0 {
                    1
                } else if factor < 0.0 {
                    -1
                } else {
                    0
                };
            return match sign {
                1 => Ok(Self::INFINITY),
                -1 => Ok(Self::NEG_INFINITY),
                _ => Err(IntervalOverflow),
            };
        }
        let months = to_i32(f64::from(self.months) * factor)?;
        let days = to_i32(f64::from(self.days) * factor)?;
        let month_remainder_days = ts_round(
            (f64::from(self.months) * factor - f64::from(months)) * f64::from(DAYS_PER_MONTH),
        );
        let mut sec_remainder = ts_round(
            (f64::from(self.days) * factor - f64::from(days) + month_remainder_days
                - month_remainder_days.trunc())
                * f64_of(SECS_PER_DAY),
        );
        cascade(
            months,
            days,
            month_remainder_days,
            &mut sec_remainder,
            || f64_of(self.micros) * factor,
        )
    }

    /// `interval_div`.
    ///
    /// # Errors
    /// Returns an overflow; division by zero is reported by the caller.
    pub fn div(&self, factor: f64) -> Result<Self, IntervalOverflow> {
        if factor.is_nan() {
            return Err(IntervalOverflow);
        }
        if self.is_infinite() {
            if factor.is_infinite() {
                return Err(IntervalOverflow);
            }
            return Ok(if factor < 0.0 { self.negate()? } else { *self });
        }
        if factor.is_infinite() {
            return Ok(Self::default());
        }
        let months = to_i32(f64::from(self.months) / factor)?;
        let days = to_i32(f64::from(self.days) / factor)?;
        let month_remainder_days = ts_round(
            (f64::from(self.months) / factor - f64::from(months)) * f64::from(DAYS_PER_MONTH),
        );
        let mut sec_remainder = ts_round(
            (f64::from(self.days) / factor - f64::from(days) + month_remainder_days
                - month_remainder_days.trunc())
                * f64_of(SECS_PER_DAY),
        );
        cascade(
            months,
            days,
            month_remainder_days,
            &mut sec_remainder,
            || f64_of(self.micros) / factor,
        )
    }

    const fn sign(&self) -> i32 {
        if self.months > 0
            || (self.months == 0 && (self.days > 0 || (self.days == 0 && self.micros > 0)))
        {
            1
        } else if self.months == 0 && self.days == 0 && self.micros == 0 {
            0
        } else {
            -1
        }
    }

    /// `interval_justify_hours`.
    ///
    /// # Errors
    /// Returns an overflow when the day count overflows.
    pub fn justify_hours(&self) -> Result<Self, IntervalOverflow> {
        if self.is_infinite() {
            return Ok(*self);
        }
        let mut result = *self;
        let whole_days = result.micros / USECS_PER_DAY;
        result.micros -= whole_days * USECS_PER_DAY;
        result.days = result
            .days
            .checked_add(i32::try_from(whole_days).map_err(|_| IntervalOverflow)?)
            .ok_or(IntervalOverflow)?;
        if result.days > 0 && result.micros < 0 {
            result.micros += USECS_PER_DAY;
            result.days -= 1;
        } else if result.days < 0 && result.micros > 0 {
            result.micros -= USECS_PER_DAY;
            result.days += 1;
        }
        Ok(result)
    }

    /// `interval_justify_days`.
    ///
    /// # Errors
    /// Returns an overflow when the month count overflows.
    pub fn justify_days(&self) -> Result<Self, IntervalOverflow> {
        if self.is_infinite() {
            return Ok(*self);
        }
        let mut result = *self;
        let whole_months = result.days / DAYS_PER_MONTH;
        result.days -= whole_months * DAYS_PER_MONTH;
        result.months = result
            .months
            .checked_add(whole_months)
            .ok_or(IntervalOverflow)?;
        if result.months > 0 && result.days < 0 {
            result.days += DAYS_PER_MONTH;
            result.months -= 1;
        } else if result.months < 0 && result.days > 0 {
            result.days -= DAYS_PER_MONTH;
            result.months += 1;
        }
        Ok(result)
    }

    /// `interval_justify_interval`.
    ///
    /// # Errors
    /// Returns an overflow when the month count overflows.
    pub fn justify_interval(&self) -> Result<Self, IntervalOverflow> {
        if self.is_infinite() {
            return Ok(*self);
        }
        let mut result = *self;
        if (result.days > 0 && result.micros > 0) || (result.days < 0 && result.micros < 0) {
            let whole_months = result.days / DAYS_PER_MONTH;
            result.days -= whole_months * DAYS_PER_MONTH;
            result.months = result
                .months
                .checked_add(whole_months)
                .ok_or(IntervalOverflow)?;
        }
        let whole_days = result.micros / USECS_PER_DAY;
        result.micros -= whole_days * USECS_PER_DAY;
        result.days += i32::try_from(whole_days).map_err(|_| IntervalOverflow)?;
        let whole_months = result.days / DAYS_PER_MONTH;
        result.days -= whole_months * DAYS_PER_MONTH;
        result.months = result
            .months
            .checked_add(whole_months)
            .ok_or(IntervalOverflow)?;
        if result.months > 0 && (result.days < 0 || (result.days == 0 && result.micros < 0)) {
            result.days += DAYS_PER_MONTH;
            result.months -= 1;
        } else if result.months < 0 && (result.days > 0 || (result.days == 0 && result.micros > 0))
        {
            result.days -= DAYS_PER_MONTH;
            result.months += 1;
        }
        if result.days > 0 && result.micros < 0 {
            result.micros += USECS_PER_DAY;
            result.days -= 1;
        } else if result.days < 0 && result.micros > 0 {
            result.micros -= USECS_PER_DAY;
            result.days += 1;
        }
        Ok(result)
    }

    /// `AdjustIntervalForTypmod`.
    ///
    /// # Errors
    /// Returns an overflow when rounding leaves the representable range.
    pub fn apply_typmod(&self, typmod: i32) -> Result<Self, IntervalOverflow> {
        if typmod < 0 || self.is_infinite() {
            return Ok(*self);
        }
        let range = (typmod >> 16) & 0x7FFF;
        let precision = typmod & 0xFFFF;
        let mut result = *self;
        let truncate = |micros: i64, unit: i64| (micros / unit) * unit;
        match range {
            YEAR_MASK => {
                result.months = (result.months / MONTHS_PER_YEAR) * MONTHS_PER_YEAR;
                result.days = 0;
                result.micros = 0;
            }
            MONTH_MASK => {
                result.days = 0;
                result.micros = 0;
            }
            range if range == YEAR_MASK | MONTH_MASK => {
                result.days = 0;
                result.micros = 0;
            }
            DAY_MASK => result.micros = 0,
            HOUR_MASK => result.micros = truncate(result.micros, USECS_PER_HOUR),
            MINUTE_MASK => result.micros = truncate(result.micros, USECS_PER_MINUTE),
            range if range == DAY_MASK | HOUR_MASK => {
                result.micros = truncate(result.micros, USECS_PER_HOUR);
            }
            range
                if range == DAY_MASK | HOUR_MASK | MINUTE_MASK
                    || range == HOUR_MASK | MINUTE_MASK =>
            {
                result.micros = truncate(result.micros, USECS_PER_MINUTE);
            }
            _ => {}
        }
        if precision != FULL_PRECISION {
            let precision = usize::try_from(precision.clamp(0, 6)).unwrap_or(6);
            result.micros =
                round_interval_time(result.micros, precision).ok_or(IntervalOverflow)?;
        }
        Ok(result)
    }

    /// `interval_out` in the given `IntervalStyle`.
    #[must_use]
    pub fn format(&self, style: IntervalStyle) -> String {
        if *self == Self::INFINITY {
            return "infinity".to_string();
        }
        if *self == Self::NEG_INFINITY {
            return "-infinity".to_string();
        }
        let parts = Parts::of(self);
        match style {
            IntervalStyle::Postgres => parts.postgres(),
            IntervalStyle::PostgresVerbose => parts.verbose(),
            IntervalStyle::SqlStandard => parts.sql_standard(),
            IntervalStyle::Iso8601 => parts.iso_8601(),
        }
    }
}

fn finite(value: Interval) -> Result<Interval, IntervalOverflow> {
    if value.is_infinite() {
        Err(IntervalOverflow)
    } else {
        Ok(value)
    }
}

fn to_i32(value: f64) -> Result<i32, IntervalOverflow> {
    if value.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        return Err(IntervalOverflow);
    }
    super::tokens::truncate_f64_i32(value).ok_or(IntervalOverflow)
}

fn f64_of(value: i64) -> f64 {
    num_traits::ToPrimitive::to_f64(&value).unwrap_or(0.0)
}

/// `TSROUND`: rounds to microsecond precision.
fn ts_round(value: f64) -> f64 {
    (value * 1_000_000.0).round_ties_even() / 1_000_000.0
}

fn cascade(
    months: i32,
    days: i32,
    month_remainder_days: f64,
    sec_remainder: &mut f64,
    time: impl Fn() -> f64,
) -> Result<Interval, IntervalOverflow> {
    let mut days = days;
    let secs_per_day = f64_of(SECS_PER_DAY);
    if sec_remainder.abs() >= secs_per_day {
        let whole = to_i32(*sec_remainder / secs_per_day)?;
        days = days.checked_add(whole).ok_or(IntervalOverflow)?;
        *sec_remainder -= f64::from(whole) * secs_per_day;
    }
    days = days
        .checked_add(to_i32(month_remainder_days)?)
        .ok_or(IntervalOverflow)?;
    let micros = (time() + *sec_remainder * f64_of(USECS_PER_SEC)).round_ties_even();
    if micros.is_nan() || !(-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&micros) {
        return Err(IntervalOverflow);
    }
    finite(Interval {
        months,
        days,
        micros: round_half_even(micros),
    })
}

/// `interval2itm`: the broken-down fields used by every output style.
struct Parts {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    micros: i64,
}

impl Parts {
    fn of(interval: &Interval) -> Self {
        let mut time = interval.micros;
        let hour = time / USECS_PER_HOUR;
        time -= hour * USECS_PER_HOUR;
        let minute = time / USECS_PER_MINUTE;
        time -= minute * USECS_PER_MINUTE;
        let second = time / USECS_PER_SEC;
        time -= second * USECS_PER_SEC;
        Self {
            year: i64::from(interval.months / MONTHS_PER_YEAR),
            month: i64::from(interval.months % MONTHS_PER_YEAR),
            day: i64::from(interval.days),
            hour,
            minute,
            second,
            micros: time,
        }
    }

    fn has_time(&self) -> bool {
        self.hour != 0 || self.minute != 0 || self.second != 0 || self.micros != 0
    }

    fn postgres(&self) -> String {
        let mut out = String::new();
        let (mut is_zero, mut is_before) = (true, false);
        for (value, unit) in [(self.year, "year"), (self.month, "mon"), (self.day, "day")] {
            if value == 0 {
                continue;
            }
            let _ = write!(
                out,
                "{}{}{value} {unit}{}",
                if is_zero { "" } else { " " },
                if is_before && value > 0 { "+" } else { "" },
                if value == 1 { "" } else { "s" }
            );
            is_before = value < 0;
            is_zero = false;
        }
        if is_zero || self.has_time() {
            let minus = self.hour < 0 || self.minute < 0 || self.second < 0 || self.micros < 0;
            let _ = write!(
                out,
                "{}{}{:02}:{:02}:",
                if is_zero { "" } else { " " },
                if minus {
                    "-"
                } else if is_before {
                    "+"
                } else {
                    ""
                },
                self.hour.unsigned_abs(),
                self.minute.unsigned_abs()
            );
            append_seconds(&mut out, self.second, self.micros, true);
        }
        out
    }

    fn verbose(&self) -> String {
        let mut out = String::from("@");
        let (mut is_zero, mut is_before) = (true, false);
        for (value, unit) in [
            (self.year, "year"),
            (self.month, "mon"),
            (self.day, "day"),
            (self.hour, "hour"),
            (self.minute, "min"),
        ] {
            if value == 0 {
                continue;
            }
            let mut value = value;
            if is_zero {
                is_before = value < 0;
                value = value.abs();
            } else if is_before {
                value = -value;
            }
            let _ = write!(out, " {value} {unit}{}", if value == 1 { "" } else { "s" });
            is_zero = false;
        }
        if self.second != 0 || self.micros != 0 {
            out.push(' ');
            if self.second < 0 || (self.second == 0 && self.micros < 0) {
                if is_zero {
                    is_before = true;
                } else if !is_before {
                    out.push('-');
                }
            } else if is_before {
                out.push('-');
            }
            append_seconds(&mut out, self.second, self.micros, false);
            out.push_str(if self.second.abs() != 1 || self.micros != 0 {
                " secs"
            } else {
                " sec"
            });
            is_zero = false;
        }
        if is_zero {
            out.push_str(" 0");
        }
        if is_before {
            out.push_str(" ago");
        }
        out
    }

    fn sql_standard(&self) -> String {
        let fields = [
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.micros,
        ];
        let has_negative = fields.iter().any(|value| *value < 0);
        let has_positive = fields.iter().any(|value| *value > 0);
        let has_year_month = self.year != 0 || self.month != 0;
        let has_day_time = self.day != 0 || self.has_time();
        let standard = !((has_negative && has_positive) || (has_year_month && has_day_time));
        let mut out = String::new();
        let sign = if has_negative && standard {
            out.push('-');
            -1
        } else {
            1
        };
        let [year, month, day, hour, minute, second, micros] = fields.map(|value| value * sign);
        if !has_negative && !has_positive {
            out.push('0');
        } else if !standard {
            let year_sign = if year < 0 || month < 0 { '-' } else { '+' };
            let day_sign = if day < 0 { '-' } else { '+' };
            let time_sign = if hour < 0 || minute < 0 || second < 0 || micros < 0 {
                '-'
            } else {
                '+'
            };
            let _ = write!(
                out,
                "{year_sign}{}-{} {day_sign}{} {time_sign}{}:{:02}:",
                year.unsigned_abs(),
                month.unsigned_abs(),
                day.unsigned_abs(),
                hour.unsigned_abs(),
                minute.unsigned_abs()
            );
            append_seconds(&mut out, second, micros, true);
        } else if has_year_month {
            let _ = write!(out, "{year}-{month}");
        } else if day != 0 {
            let _ = write!(out, "{day} {hour}:{minute:02}:");
            append_seconds(&mut out, second, micros, true);
        } else {
            let _ = write!(out, "{hour}:{minute:02}:");
            append_seconds(&mut out, second, micros, true);
        }
        out
    }

    fn iso_8601(&self) -> String {
        if self.year == 0 && self.month == 0 && self.day == 0 && !self.has_time() {
            return "PT0S".to_string();
        }
        let mut out = String::from("P");
        for (value, unit) in [(self.year, 'Y'), (self.month, 'M'), (self.day, 'D')] {
            if value != 0 {
                let _ = write!(out, "{value}{unit}");
            }
        }
        if self.has_time() {
            out.push('T');
        }
        for (value, unit) in [(self.hour, 'H'), (self.minute, 'M')] {
            if value != 0 {
                let _ = write!(out, "{value}{unit}");
            }
        }
        if self.second != 0 || self.micros != 0 {
            if self.second < 0 || self.micros < 0 {
                out.push('-');
            }
            append_seconds(&mut out, self.second, self.micros, false);
            out.push('S');
        }
        out
    }
}

/// `AppendSeconds`: seconds with trailing-zero-trimmed microseconds.
pub fn append_seconds(out: &mut String, second: i64, micros: i64, fill_zeros: bool) {
    if fill_zeros {
        let _ = write!(out, "{:02}", second.unsigned_abs());
    } else {
        out.push_str(&second.unsigned_abs().to_string());
    }
    if micros != 0 {
        let fraction = format!("{:06}", micros.unsigned_abs());
        out.push('.');
        out.push_str(fraction.trim_end_matches('0'));
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_interval, Interval, IntervalStyle, FULL_RANGE};

    fn interval(text: &str) -> Interval {
        parse_interval(text, FULL_RANGE, IntervalStyle::Postgres).expect("valid interval")
    }

    #[test]
    fn should_format_intervals_in_every_style() {
        // Arrange
        let value = interval("1 year 2 mons 3 days 04:05:06.7");

        // Act
        let styles = [
            IntervalStyle::Postgres,
            IntervalStyle::PostgresVerbose,
            IntervalStyle::SqlStandard,
            IntervalStyle::Iso8601,
        ]
        .map(|style| value.format(style));

        // Assert
        assert_eq!(
            styles,
            [
                "1 year 2 mons 3 days 04:05:06.7",
                "@ 1 year 2 mons 3 days 4 hours 5 mins 6.7 secs",
                "+1-2 +3 +4:05:06.7",
                "P1Y2M3DT4H5M6.7S"
            ]
        );
    }

    #[test]
    fn should_scale_and_justify_intervals_like_postgresql() {
        // Arrange
        let day = interval("1 day");

        // Act
        let scaled = day.mul(2.5).expect("product");
        let divided = interval("3 hours").div(1.5).expect("quotient");
        let justified = interval("1 month -1 hour")
            .justify_interval()
            .expect("justified");

        // Assert
        assert_eq!(scaled, interval("2 days 12 hours"));
        assert_eq!(divided, interval("2 hours"));
        assert_eq!(
            justified.format(IntervalStyle::Postgres),
            "29 days 23:00:00"
        );
    }
}
