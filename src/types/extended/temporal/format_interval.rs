//! Interval-specific `to_char` day-of-year and week calculations.

use super::super::calendar::Fields;

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
