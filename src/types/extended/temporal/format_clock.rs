//! Clock fields for temporal `to_char` formatting.

use super::{case_like, fraction, interval, number, Moment};

pub(super) fn render_clock_field(
    moment: &Moment,
    keyword: &str,
    spelling: &str,
    fill: bool,
    hour: i64,
) -> Option<(String, Option<i64>)> {
    let fields = moment.fields;
    let numeric = |value: i64, width: usize| (number(value, width, fill), Some(value));
    Some(match keyword {
        "hh" | "hh12" => {
            if moment.is_interval {
                interval::hour12(hour, fill)
            } else {
                numeric((hour + 11) % 12 + 1, 2)
            }
        }
        "hh24" => numeric(hour, 2),
        "mi" => numeric(i64::from(fields.minute), 2),
        "ss" => numeric(i64::from(fields.second), 2),
        "ms" => fraction::field(fields.micros, 3),
        "us" => fraction::field(fields.micros, 6),
        "ff1" | "ff2" | "ff3" | "ff4" | "ff5" | "ff6" => {
            let digits = usize::from(keyword.as_bytes()[2] - b'0');
            fraction::field(fields.micros, digits)
        }
        "ssss" | "sssss" => numeric(
            hour * 3600 + i64::from(fields.minute) * 60 + i64::from(fields.second),
            0,
        ),
        "am" | "pm" | "a.m." | "p.m." => {
            let pm = hour % 24 >= 12;
            let text = match (keyword.len() > 2, pm) {
                (false, false) => "am",
                (false, true) => "pm",
                (true, false) => "a.m.",
                (true, true) => "p.m.",
            };
            (case_like(spelling, text), None)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::super::calendar::Fields;
    use super::super::{to_char, Moment};

    #[test]
    fn should_preserve_public_moment_hour_formatting() {
        // Arrange
        let moment = Moment {
            fields: Fields {
                year: 0,
                month: 0,
                day: 0,
                hour: -12,
                minute: 0,
                second: 0,
                micros: 0,
            },
            zone_east: None,
            is_interval: true,
        };

        // Act
        let output = to_char(&moment, "HH12|HH24|SSSS", "");

        // Assert
        assert_eq!(output, "012|-12|-43200");
    }
}
