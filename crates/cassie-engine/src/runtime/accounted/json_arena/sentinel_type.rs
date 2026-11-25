//! Structured unexpected-type diagnostics for string-valued sentinel fields.
use super::{Document, DocumentError, DocumentProgress, Literal, Position, SyntaxKind};

#[derive(Debug, Clone, Copy)]
pub(super) enum Expected {
    NumberString,
    RawValue,
}

#[derive(Debug, Clone, Copy)]
enum Unexpected {
    Null,
    Bool(bool),
    Unsigned(u64),
    Signed(i64),
    Number,
    Sequence,
    Map,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::runtime::accounted::json_arena) struct TypeDiagnostic {
    unexpected: Unexpected,
    expected: Expected,
    position: Position,
}

impl TypeDiagnostic {
    pub(super) fn write_message(&self, out: &mut impl std::fmt::Write) -> std::fmt::Result {
        out.write_str("invalid type: ")?;
        match self.unexpected {
            Unexpected::Null => out.write_str("null")?,
            Unexpected::Bool(value) => write!(out, "boolean `{value}`")?,
            Unexpected::Unsigned(value) => write!(out, "integer `{value}`")?,
            Unexpected::Signed(value) => write!(out, "integer `{value}`")?,
            Unexpected::Number => out.write_str("number")?,
            Unexpected::Sequence => out.write_str("sequence")?,
            Unexpected::Map => out.write_str("map")?,
        }
        let expected = match self.expected {
            Expected::NumberString => "string containing a number",
            Expected::RawValue => "raw value",
        };
        write!(
            out,
            ", expected {expected} at line {} column {}",
            self.position.line, self.position.column
        )
    }
    #[cfg(test)]
    pub(super) fn test_info(&self) -> (serde_json::error::Category, Position) {
        (serde_json::error::Category::Data, self.position)
    }

    #[cfg(test)]
    pub(super) fn test_message(&self) -> String {
        let unexpected = match self.unexpected {
            Unexpected::Null => "null".to_owned(),
            Unexpected::Bool(value) => format!("boolean `{value}`"),
            Unexpected::Unsigned(value) => format!("integer `{value}`"),
            Unexpected::Signed(value) => format!("integer `{value}`"),
            Unexpected::Number => "number".to_owned(),
            Unexpected::Sequence => "sequence".to_owned(),
            Unexpected::Map => "map".to_owned(),
        };
        let expected = match self.expected {
            Expected::NumberString => "string containing a number",
            Expected::RawValue => "raw value",
        };
        format!(
            "invalid type: {unexpected}, expected {expected} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

impl Document<'_> {
    pub(super) fn unexpected_sentinel_type(
        &mut self,
        expected: Expected,
        observer: &mut impl FnMut(DocumentProgress),
    ) -> Result<DocumentError, DocumentError> {
        let (unexpected, position) = match self.whitespace()? {
            None => return self.syntax(SyntaxKind::EofValue),
            Some(b'n' | b't' | b'f') => {
                let (literal, position) = self.sources.step(|cursor| {
                    let literal = cursor.read_literal()?;
                    Ok((literal, cursor.position()))
                })?;
                let value = match literal {
                    Literal::Null => Unexpected::Null,
                    Literal::True => Unexpected::Bool(true),
                    Literal::False => Unexpected::Bool(false),
                };
                (value, position)
            }
            Some(b'-' | b'0'..=b'9') => {
                let (number, position) = self.sources.step(|cursor| {
                    let number = cursor.read_number()?;
                    Ok((number, cursor.position()))
                })?;
                let value = classify_number(number.as_str());
                observer(DocumentProgress::NumberTypeScanned);
                self.controls.check_interruption()?;
                drop(number);
                (value, position)
            }
            Some(b'[') => (
                Unexpected::Sequence,
                self.sources.step(|cursor| Ok(cursor.position()))?,
            ),
            Some(b'{') => (
                Unexpected::Map,
                self.sources.step(|cursor| Ok(cursor.position()))?,
            ),
            Some(_) => return self.syntax(SyntaxKind::ExpectedValue),
        };
        Ok(DocumentError::SentinelType(TypeDiagnostic {
            unexpected,
            expected,
            position,
        }))
    }
}

fn classify_number(text: &str) -> Unexpected {
    if text == "-0" || text.len() > 20 {
        return Unexpected::Number;
    }
    if text.starts_with('-') {
        text.parse::<i64>()
            .map_or(Unexpected::Number, Unexpected::Signed)
    } else {
        text.parse::<u64>()
            .map_or(Unexpected::Number, Unexpected::Unsigned)
    }
}

#[cfg(test)]
mod tests {
    use super::{Expected, TypeDiagnostic, Unexpected};
    use crate::runtime::accounted::json_arena::cursor::Position;

    #[test]
    fn should_match_the_raw_value_expected_phrase() {
        // Arrange
        let input = r#"{"$serde_json::private::RawValue":null}"#;
        let expected = serde_json::from_str::<serde_json::Value>(input).expect_err("serde");
        let diagnostic = TypeDiagnostic {
            unexpected: Unexpected::Null,
            expected: Expected::RawValue,
            position: Position {
                offset: 0,
                line: expected.line(),
                column: expected.column(),
            },
        };

        // Act / Assert
        // Assert
        assert_eq!(diagnostic.test_message(), expected.to_string());
    }
}
