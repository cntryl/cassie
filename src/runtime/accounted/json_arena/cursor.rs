//! Checked, on-demand source cursor; no tokenizer or consumer is wired yet.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;

#[path = "literal.rs"]
pub(super) mod literal;
#[path = "number.rs"]
pub(super) mod number;
#[path = "string.rs"]
pub(super) mod string;

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Position {
    pub(super) offset: usize,
    pub(super) line: usize,
    pub(super) column: usize,
}

#[derive(Debug)]
pub(super) enum CursorError {
    InvalidUtf8,
    Interrupted(CassieError),
    Syntax(string::Diagnostic),
    Number(number::NumberDiagnostic),
    Literal(literal::LiteralDiagnostic),
}

#[cfg(test)]
impl CursorError {
    pub(super) fn test_info(&self) -> Option<(serde_json::error::Category, Position)> {
        match self {
            Self::Syntax(error) => Some(error.test_info()),
            Self::Number(error) => Some(error.test_info()),
            Self::Literal(error) => Some(error.test_info()),
            Self::InvalidUtf8 | Self::Interrupted(_) => None,
        }
    }

    pub(super) fn test_message(&self) -> Option<String> {
        match self {
            Self::Syntax(error) => Some(error.test_message()),
            Self::Number(error) => Some(error.test_message()),
            Self::Literal(error) => Some(error.test_message()),
            Self::InvalidUtf8 | Self::Interrupted(_) => None,
        }
    }
}

pub(super) struct Cursor<'a> {
    source: &'a [u8],
    position: Position,
    controls: QueryExecutionControls,
    #[cfg(test)]
    row_bytes: bool,
}

impl<'a> Cursor<'a> {
    #[cfg(test)]
    pub(super) fn from_row_bytes(source: &'a [u8], controls: &QueryExecutionControls) -> Self {
        let mut cursor = Self::new(source, controls);
        cursor.row_bytes = true;
        cursor
    }

    #[cfg(test)]
    pub(super) fn enable_row_bytes(&mut self) {
        self.row_bytes = true;
    }

    pub(super) fn from_str(source: &'a str, controls: &QueryExecutionControls) -> Self {
        Self::new(source.as_bytes(), controls)
    }

    pub(super) fn from_bytes(
        source: &'a [u8],
        controls: &QueryExecutionControls,
    ) -> Result<Self, CursorError> {
        Self::from_bytes_with_observer(source, controls, || {})
    }

    #[cfg(test)]
    fn from_bytes_observed(
        source: &'a [u8],
        controls: &QueryExecutionControls,
        observer: impl FnMut(),
    ) -> Result<Self, CursorError> {
        Self::from_bytes_with_observer(source, controls, observer)
    }

    fn from_bytes_with_observer(
        source: &'a [u8],
        controls: &QueryExecutionControls,
        mut observer: impl FnMut(),
    ) -> Result<Self, CursorError> {
        let mut start = 0;
        controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        while start < source.len() {
            controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
            let end = start.saturating_add(1024).min(source.len());
            match std::str::from_utf8(&source[start..end]) {
                Ok(_) => start = end,
                Err(error) if error.error_len().is_none() && end < source.len() => {
                    let valid = error.valid_up_to();
                    if valid == 0 {
                        return Err(CursorError::InvalidUtf8);
                    }
                    start += valid;
                }
                Err(_) => return Err(CursorError::InvalidUtf8),
            }
            observer();
            controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
        }
        Ok(Self::new(source, controls))
    }

    fn new(source: &'a [u8], controls: &QueryExecutionControls) -> Self {
        Self {
            source,
            position: Position {
                offset: 0,
                line: 1,
                column: 0,
            },
            controls: controls.clone(),
            #[cfg(test)]
            row_bytes: false,
        }
    }

    pub(super) fn restore_validated(
        source: &'a [u8],
        position: Position,
        controls: &QueryExecutionControls,
    ) -> Self {
        Self {
            source,
            position,
            controls: controls.clone(),
            #[cfg(test)]
            row_bytes: false,
        }
    }

    pub(super) fn position(&self) -> Position {
        self.position
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.source.get(self.position.offset).copied()
    }

    pub(super) fn peek_position(&self) -> Result<Position, CursorError> {
        let Some(byte) = self.peek() else {
            return Ok(self.position);
        };
        self.position_after(byte)
    }

    pub(super) fn next(&mut self) -> Result<Option<u8>, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        let Some(byte) = self.peek() else {
            return Ok(None);
        };
        self.position = self.position_after(byte)?;
        Ok(Some(byte))
    }

    fn position_after(&self, byte: u8) -> Result<Position, CursorError> {
        let overflow = || {
            CursorError::Interrupted(CassieError::ResourceLimit(
                "JSON cursor position overflow".into(),
            ))
        };
        let offset = self.position.offset.checked_add(1).ok_or_else(overflow)?;
        if byte == b'\n' {
            Ok(Position {
                offset,
                line: self.position.line.checked_add(1).ok_or_else(overflow)?,
                column: 0,
            })
        } else {
            Ok(Position {
                offset,
                line: self.position.line,
                column: self.position.column.checked_add(1).ok_or_else(overflow)?,
            })
        }
    }

    pub(super) fn skip_whitespace(&mut self) -> Result<Option<u8>, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\t' | b'\r')) {
            let _ = self.next()?;
        }
        Ok(self.peek())
    }
}
