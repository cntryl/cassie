//! Decode one reached raw JSON string while retaining its admitted owner.
use std::fmt;

use crate::app::CassieError;
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;

use super::cursor::{Cursor, CursorError, Position};

#[cfg(test)]
#[path = "raw_decode_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) struct DecodeError(CursorError);

#[derive(Clone, Copy)]
pub(crate) struct RawPosition {
    pub(crate) offset: usize,
    pub(crate) line: usize,
    pub(crate) column: usize,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            CursorError::Syntax(error) => error.write_message(out),
            CursorError::Interrupted(error) => write!(out, "{error}"),
            _ => out.write_str("invalid JSON string"),
        }
    }
}

impl DecodeError {
    pub(crate) fn into_control(self) -> Result<CassieError, Self> {
        match self.0 {
            CursorError::Interrupted(error) => Ok(error),
            other => Err(Self(other)),
        }
    }
}

pub(crate) fn decode(
    raw: &str,
    controls: &QueryExecutionControls,
) -> Result<Accounted<String>, DecodeError> {
    decode_at(
        raw,
        RawPosition {
            offset: 0,
            line: 1,
            column: 0,
        },
        controls,
    )
}

pub(crate) fn decode_at(
    source: &str,
    position: RawPosition,
    controls: &QueryExecutionControls,
) -> Result<Accounted<String>, DecodeError> {
    // The caller supplies a position reached by the checked raw scanner; the
    // character-boundary check also prevents creating a cursor inside UTF-8.
    if !source.is_char_boundary(position.offset) {
        return Err(DecodeError(CursorError::Interrupted(
            CassieError::Execution("invalid JSON key position".into()),
        )));
    }
    let mut cursor = Cursor::restore_validated(
        source.as_bytes(),
        Position {
            offset: position.offset,
            line: position.line,
            column: position.column,
        },
        controls,
    );
    let value = cursor.read_string().map_err(DecodeError)?;
    let (text, memory) = value.into_parts();
    Ok(Accounted::from_admitted(text, memory))
}

#[cfg(test)]
fn decode_observing(
    raw: &str,
    controls: &QueryExecutionControls,
    observer: impl FnMut(),
) -> Result<Accounted<String>, DecodeError> {
    let mut cursor = Cursor::from_str(raw, controls);
    let value = cursor.read_string_observed(observer).map_err(DecodeError)?;
    let (text, memory) = value.into_parts();
    Ok(Accounted::from_admitted(text, memory))
}
