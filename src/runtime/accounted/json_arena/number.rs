//! On-demand arbitrary-precision Value and strict number tokens.
use super::string::DecodedText;
use super::{Cursor, CursorError, Position};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "number_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumberError {
    Eof,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::accounted::json_arena) struct NumberDiagnostic {
    kind: NumberError,
    position: Position,
}

impl NumberDiagnostic {
    pub(in crate::runtime::accounted::json_arena) fn write_message(
        &self,
        out: &mut impl std::fmt::Write,
    ) -> std::fmt::Result {
        let message = match self.kind {
            NumberError::Eof => "EOF while parsing a value",
            NumberError::Invalid => "invalid number",
        };
        write!(
            out,
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

pub(in crate::runtime::accounted::json_arena) struct NumberToken(DecodedText);

impl NumberToken {
    pub(in crate::runtime::accounted::json_arena) fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(in crate::runtime::accounted::json_arena) fn shares_tracker(
        &self,
        controls: &QueryExecutionControls,
    ) -> bool {
        self.0.shares_tracker(controls)
    }

    pub(in crate::runtime::accounted::json_arena) fn into_parts(
        self,
    ) -> (String, QueryMemoryReservation) {
        self.0.into_parts()
    }
}

#[cfg(test)]
impl NumberDiagnostic {
    pub(in crate::runtime::accounted::json_arena) fn test_info(
        &self,
    ) -> (serde_json::error::Category, Position) {
        let category = if self.kind == NumberError::Eof {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        (category, self.position)
    }

    pub(in crate::runtime::accounted::json_arena) fn test_message(&self) -> String {
        let message = match self.kind {
            NumberError::Eof => "EOF while parsing a value",
            NumberError::Invalid => "invalid number",
        };
        format!(
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

impl Cursor<'_> {
    pub(in crate::runtime::accounted::json_arena) fn read_number(
        &mut self,
    ) -> Result<NumberToken, CursorError> {
        self.read_number_with_observer(|| {})
    }

    #[cfg(test)]
    fn read_number_observed(&mut self, observer: impl FnMut()) -> Result<NumberToken, CursorError> {
        self.read_number_with_observer(observer)
    }

    pub(in crate::runtime::accounted::json_arena) fn read_strict_number(
        &mut self,
    ) -> Result<NumberToken, CursorError> {
        let result = self.read_number();
        if matches!(
            &result,
            Err(CursorError::Interrupted(_) | CursorError::InvalidUtf8 | CursorError::Syntax(_))
        ) {
            return result;
        }
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        if self.peek().is_some() {
            drop(result);
            return Err(self.number_peek_error(NumberError::Invalid)?);
        }
        result
    }

    fn read_number_with_observer(
        &mut self,
        mut observer: impl FnMut(),
    ) -> Result<NumberToken, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        match self.peek() {
            None => return Err(self.number_peek_error(NumberError::Eof)?),
            Some(b'-' | b'0'..=b'9') => {}
            Some(_) => return Err(self.number_peek_error(NumberError::Invalid)?),
        }
        let mut text = DecodedText::new(&self.controls)?;
        if self.peek() == Some(b'-') {
            let _ = self.next()?;
            text.push('-', &self.controls, &mut observer)?;
        }
        let first = self.number_required()?;
        match first {
            b'0' => {
                text.push('0', &self.controls, &mut observer)?;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.number_peek_error(NumberError::Invalid)?);
                }
            }
            b'1'..=b'9' => {
                text.push(char::from(first), &self.controls, &mut observer)?;
                self.number_digits(&mut text, &mut observer)?;
            }
            _ => return Err(self.number_error(NumberError::Invalid)),
        }
        if self.peek() == Some(b'.') {
            let _ = self.next()?;
            text.push('.', &self.controls, &mut observer)?;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                let kind = if self.peek().is_some() {
                    NumberError::Invalid
                } else {
                    NumberError::Eof
                };
                return Err(self.number_peek_error(kind)?);
            }
            self.number_digits(&mut text, &mut observer)?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let _ = self.next()?;
            text.push('e', &self.controls, &mut observer)?;
            match self.peek() {
                Some(sign @ (b'+' | b'-')) => {
                    let _ = self.next()?;
                    text.push(char::from(sign), &self.controls, &mut observer)?;
                }
                _ => text.push('+', &self.controls, &mut observer)?,
            }
            let first_exponent = self.number_required()?;
            if !first_exponent.is_ascii_digit() {
                return Err(self.number_error(NumberError::Invalid));
            }
            text.push(char::from(first_exponent), &self.controls, &mut observer)?;
            self.number_digits(&mut text, &mut observer)?;
        }
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        Ok(NumberToken(text))
    }

    fn number_digits(
        &mut self,
        text: &mut DecodedText,
        observer: &mut impl FnMut(),
    ) -> Result<(), CursorError> {
        while let Some(digit @ b'0'..=b'9') = self.peek() {
            let _ = self.next()?;
            text.push(char::from(digit), &self.controls, observer)?;
        }
        Ok(())
    }

    fn number_required(&mut self) -> Result<u8, CursorError> {
        self.next()?
            .ok_or_else(|| self.number_error(NumberError::Eof))
    }

    fn number_error(&self, kind: NumberError) -> CursorError {
        CursorError::Number(NumberDiagnostic {
            kind,
            position: self.position(),
        })
    }

    fn number_peek_error(&self, kind: NumberError) -> Result<CursorError, CursorError> {
        Ok(CursorError::Number(NumberDiagnostic {
            kind,
            position: self.peek_position()?,
        }))
    }
}
