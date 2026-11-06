//! Formatting-field timezone input retains its sign independently of hours.

use crate::types::extended::TypeInputError;

#[derive(Default)]
pub(super) struct ZoneFields {
    hours: i64,
    minutes: i64,
    negative: bool,
    present: bool,
}

impl ZoneFields {
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
