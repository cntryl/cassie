//! Format a number token already accepted by the raw JSON scanner.
use crate::app::CassieError;
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;

use super::forest::jsonb_number::{self, FormatError};

pub(crate) enum RawNumberError {
    Overflow,
    Control(CassieError),
}

pub(crate) fn format(
    token: &str,
    controls: &QueryExecutionControls,
) -> Result<Accounted<String>, RawNumberError> {
    let text =
        jsonb_number::format_number_with_observer(token, controls, |_| {}).map_err(|error| {
            match error {
                FormatError::Overflow => RawNumberError::Overflow,
                FormatError::Control(error) => RawNumberError::Control(error),
            }
        })?;
    let (text, memory) = text.into_parts();
    Ok(Accounted::from_admitted(text, memory))
}
