//! Interval-specific `to_char` field calculations.

use super::super::calendar::Fields;

pub(super) fn roman_month(fields: Fields) -> usize {
    let month = i64::from(fields.month);
    let index = if month == 0 {
        if fields.year < 0 {
            0
        } else {
            11
        }
    } else if month < 0 {
        month + 12
    } else {
        month - 1
    };
    usize::try_from(index).unwrap_or(0).min(11)
}

pub(super) fn hour12(hour: i64, fill: bool) -> (String, Option<i64>) {
    let remainder = hour % 12;
    let value = if remainder == 0 { 12 } else { remainder };
    let text = if fill {
        value.to_string()
    } else {
        let width = if hour < 0 { 3 } else { 2 };
        format!("{value:0width$}")
    };
    (text, Some(value))
}

/// PostgreSQL's `fmt_tm` uses signed 32-bit fields, with wrapping arithmetic in
/// the pinned PostgreSQL build. The width includes a negative sign.
pub(super) fn span_field(fields: Fields, week: bool, fill: bool) -> (String, Option<i64>) {
    let days = fields
        .year
        .wrapping_mul(12)
        .wrapping_add(fields.month)
        .wrapping_mul(30)
        .wrapping_add(fields.day);
    let value = if week {
        days.wrapping_sub(1) / 7 + 1
    } else {
        days
    };
    let text = if fill {
        value.to_string()
    } else {
        let width = if week { 2 } else { 3 };
        format!("{value:0width$}")
    };
    (text, Some(i64::from(value)))
}
