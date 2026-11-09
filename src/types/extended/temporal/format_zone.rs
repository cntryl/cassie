//! Temporal format-field timezone input and output.

use super::catalog::ZoneAbbreviation;
use crate::types::extended::TypeInputError;

pub(super) fn format_offset(east: i32, fill: bool) -> String {
    let sign = if east < 0 { '-' } else { '+' };
    let magnitude = east.unsigned_abs();
    let hours = magnitude / 3600;
    let minutes = magnitude / 60 % 60;
    match (fill, magnitude % 3600 == 0) {
        (true, true) => format!("{sign}{hours}"),
        (false, true) => format!("{sign}{hours:02}"),
        (true, false) => format!("{sign}{hours}:{minutes:02}"),
        (false, false) => format!("{sign}{hours:02}:{minutes:02}"),
    }
}

#[derive(Default)]
pub(super) struct ZoneFields {
    hours: i64,
    minutes: i64,
    negative: bool,
    present: bool,
    abbreviation: Option<ZoneAbbreviation>,
}

impl ZoneFields {
    pub(super) fn set_abbreviation(&mut self, abbreviation: ZoneAbbreviation) {
        self.present = false;
        self.abbreviation = Some(abbreviation);
    }

    pub(super) fn abbreviation(&self) -> Option<ZoneAbbreviation> {
        if self.present {
            None
        } else {
            self.abbreviation
        }
    }

    pub(super) fn set_hours(&mut self, hours: i64, negative: bool) -> Result<(), TypeInputError> {
        self.negative = negative;
        self.present = true;
        set_field(&mut self.hours, hours, "TZH")
    }

    pub(super) fn set_minutes(&mut self, minutes: i64) -> Result<(), TypeInputError> {
        self.present = true;
        set_field(&mut self.minutes, minutes, "TZM")
    }

    pub(super) fn offset_east(&self) -> Result<Option<i32>, TypeInputError> {
        if !self.present {
            return Ok(None);
        }
        if !(0..=15).contains(&self.hours) || !(0..60).contains(&self.minutes) {
            return Err(TypeInputError {
                sqlstate: "22009",
                message: "time zone displacement out of range".into(),
            });
        }
        let seconds =
            i32::try_from(self.hours * 3600 + self.minutes * 60).map_err(|_| TypeInputError {
                sqlstate: "22009",
                message: "time zone displacement out of range".into(),
            })?;
        Ok(Some(if self.negative { -seconds } else { seconds }))
    }
}

fn set_field(slot: &mut i64, value: i64, name: &str) -> Result<(), TypeInputError> {
    if *slot != 0 && *slot != value {
        return Err(TypeInputError {
            sqlstate: "22007",
            message: format!("conflicting values for \"{name}\" field in formatting string"),
        });
    }
    *slot = value;
    Ok(())
}

#[cfg(test)]
mod output_tests {
    use super::format_offset;

    #[test]
    fn should_format_offset_sign_minutes_and_fill_without_seconds() {
        // Arrange
        let cases = [
            (0, "+00", "+0"),
            (30, "+00:00", "+0:00"),
            (-30, "-00:00", "-0:00"),
            (19_270, "+05:21", "+5:21"),
            (-17_762, "-04:56", "-4:56"),
            (-18_000, "-05", "-5"),
        ];

        for (east, padded, filled) in cases {
            // Act
            let normal = format_offset(east, false);
            let fill = format_offset(east, true);

            // Assert
            assert_eq!(normal, padded);
            assert_eq!(fill, filled);
        }
    }
}
