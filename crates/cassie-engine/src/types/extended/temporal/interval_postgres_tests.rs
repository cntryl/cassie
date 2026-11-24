use std::fmt::Write as _;

use super::{append_seconds, Interval, Parts};

fn prior_postgres(interval: &Interval) -> String {
    if *interval == Interval::INFINITY {
        return "infinity".into();
    }
    if *interval == Interval::NEG_INFINITY {
        return "-infinity".into();
    }
    let fields = Parts::of(interval);
    let mut out = String::new();
    let (mut is_zero, mut is_before) = (true, false);
    for (value, unit) in [
        (fields.year, "year"),
        (fields.month, "mon"),
        (fields.day, "day"),
    ] {
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
    if is_zero || fields.has_time() {
        let minus = fields.hour < 0 || fields.minute < 0 || fields.second < 0 || fields.micros < 0;
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
            fields.hour.unsigned_abs(),
            fields.minute.unsigned_abs()
        );
        append_seconds(&mut out, fields.second, fields.micros, true);
    }
    out
}

#[test]
fn should_preserve_postgres_interval_writer_for_signed_extremes() {
    // Arrange
    let cases = [
        (0, 0, 0),
        (1, -1, 13),
        (-1, 1, -13),
        (i64::MIN, i32::MIN, i32::MIN),
        (i64::MAX, i32::MAX, i32::MAX),
        (i64::MIN, i32::MAX, i32::MIN),
        (i64::MAX, i32::MIN, i32::MAX),
        (i64::MIN + 1, i32::MIN, i32::MIN),
    ];
    // Act / Assert
    for (micros, days, months) in cases {
        let interval = Interval {
            months,
            days,
            micros,
        };
        let mut output = String::new();
        interval.format_postgres_to(&mut output).expect("format");
        // Assert
        assert_eq!(output, prior_postgres(&interval));
    }
}
