//! On-demand null and boolean tokens for the isolated JSON cursor.
use super::{Cursor, CursorError, Position};

#[cfg(test)]
#[path = "literal_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::accounted::json_arena) enum Literal {
    Null,
    True,
    False,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiteralError {
    Eof,
    ExpectedValue,
    ExpectedIdent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::accounted::json_arena) struct LiteralDiagnostic {
    kind: LiteralError,
    position: Position,
}

impl LiteralDiagnostic {
    pub(in crate::runtime::accounted::json_arena) fn write_message(
        &self,
        out: &mut impl std::fmt::Write,
    ) -> std::fmt::Result {
        let message = match self.kind {
            LiteralError::Eof => "EOF while parsing a value",
            LiteralError::ExpectedValue => "expected value",
            LiteralError::ExpectedIdent => "expected ident",
        };
        write!(
            out,
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

#[cfg(test)]
impl LiteralDiagnostic {
    pub(super) fn test_info(&self) -> (serde_json::error::Category, Position) {
        let category = if self.kind == LiteralError::Eof {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        (category, self.position)
    }

    pub(super) fn test_message(&self) -> String {
        let message = match self.kind {
            LiteralError::Eof => "EOF while parsing a value",
            LiteralError::ExpectedValue => "expected value",
            LiteralError::ExpectedIdent => "expected ident",
        };
        format!(
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

impl Cursor<'_> {
    pub(in crate::runtime::accounted::json_arena) fn read_literal(
        &mut self,
    ) -> Result<Literal, CursorError> {
        self.read_literal_with_observer(|| {})
    }

    #[cfg(test)]
    fn read_literal_observed(&mut self, observer: impl FnMut()) -> Result<Literal, CursorError> {
        self.read_literal_with_observer(observer)
    }

    fn read_literal_with_observer(
        &mut self,
        mut observer: impl FnMut(),
    ) -> Result<Literal, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        let (expected, value): (&[u8], Literal) = match self.peek() {
            Some(b't') => (b"true", Literal::True),
            Some(b'f') => (b"false", Literal::False),
            Some(b'n') => (b"null", Literal::Null),
            None => {
                return Err(CursorError::Literal(LiteralDiagnostic {
                    kind: LiteralError::Eof,
                    position: self.peek_position()?,
                }))
            }
            Some(_) => {
                return Err(CursorError::Literal(LiteralDiagnostic {
                    kind: LiteralError::ExpectedValue,
                    position: self.peek_position()?,
                }))
            }
        };
        for expected_byte in expected {
            let Some(actual) = self.next()? else {
                return Err(CursorError::Literal(LiteralDiagnostic {
                    kind: LiteralError::Eof,
                    position: self.position(),
                }));
            };
            observer();
            self.controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
            if actual != *expected_byte {
                return Err(CursorError::Literal(LiteralDiagnostic {
                    kind: LiteralError::ExpectedIdent,
                    position: self.position(),
                }));
            }
        }
        Ok(value)
    }
}
