//! Fixed-width fractional-second fields in temporal `to_char` templates.

pub(super) fn field(micros: i64, digits: usize) -> (String, Option<i64>) {
    let divisor = 10_i64.pow(u32::try_from(6 - digits).unwrap_or(0));
    let value = micros / divisor;
    (format!("{value:0digits$}"), Some(value))
}
