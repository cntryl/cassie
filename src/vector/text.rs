//! Known-width vector text validation shared by wire output and SQL writes.
//!
//! Tokens borrow immutable input; each numeric token uses std's f64 parser and
//! the existing strict finite f32 conversion. Callers first validate with a
//! no-op callback, then allocate their known-width output and replay the text.

use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::Deserializer as _;
use serde_json::value::RawValue;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TextVectorError {
    Shape,
    Range,
}

// A callback may observe a valid prefix before later input is rejected. Retained
// output consumers must complete a no-op validation pass before construction.
pub(crate) fn visit_components_with_dimension(
    text: &str,
    expected_dimensions: Option<usize>,
    component: impl FnMut(f32),
) -> Result<usize, TextVectorError> {
    let mut range_error = false;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let dimensions = deserializer
        .deserialize_seq(TextVectorVisitor {
            expected_dimensions,
            component,
            range_error: &mut range_error,
        })
        .map_err(|_| TextVectorError::Shape)?;
    deserializer.end().map_err(|_| TextVectorError::Shape)?;
    if range_error {
        Err(TextVectorError::Range)
    } else {
        Ok(dimensions)
    }
}

struct TextVectorVisitor<'a, F> {
    expected_dimensions: Option<usize>,
    component: F,
    range_error: &'a mut bool,
}

impl<'de, F: FnMut(f32)> Visitor<'de> for TextVectorVisitor<'_, F> {
    type Value = usize;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.expected_dimensions {
            Some(dimensions) => write!(formatter, "a finite vector of {dimensions} components"),
            None => formatter.write_str("a finite vector of 1 through 16000 components"),
        }
    }

    fn visit_seq<A>(mut self, mut sequence: A) -> Result<usize, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut dimensions = 0;
        while let Some(token) = sequence.next_element::<&'de RawValue>()? {
            dimensions += 1;
            if dimensions > 16_000
                || self
                    .expected_dimensions
                    .is_some_and(|expected| dimensions > expected)
            {
                return Err(<A::Error as de::Error>::custom("vector dimension mismatch"));
            }
            let value = token
                .get()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| <A::Error as de::Error>::custom("invalid vector component"))?;
            if let Some(value) = super::f64_to_finite_f32(value) {
                (self.component)(value);
            } else {
                // Preserve SQL write precedence: wrong shape/width/syntax is a
                // shape error even if a numeric prefix exceeds the f32 range.
                *self.range_error = true;
            }
        }
        if dimensions == 0
            || self
                .expected_dimensions
                .is_some_and(|expected| dimensions != expected)
        {
            return Err(<A::Error as de::Error>::custom("vector dimension mismatch"));
        }
        Ok(dimensions)
    }
}

#[cfg(test)]
mod tests {
    use super::{visit_components_with_dimension, TextVectorError};

    #[test]
    fn should_infer_dimensions_for_bare_vector_text() {
        // Arrange
        let mut observed = Vec::new();

        // Act
        let dimensions =
            visit_components_with_dimension("[1,2.5]", None, |value| observed.push(value))
                .expect("variable-width finite vector");

        // Assert
        assert_eq!(dimensions, 2);
        assert_eq!(observed, vec![1.0, 2.5]);
    }

    #[test]
    fn should_reject_empty_and_oversized_bare_vector_text() {
        // Arrange
        let oversized = format!(
            "[{}]",
            std::iter::repeat_n("0", 16_001)
                .collect::<Vec<_>>()
                .join(",")
        );

        // Act
        let empty = visit_components_with_dimension("[]", None, |_| {});
        let too_large = visit_components_with_dimension(&oversized, None, |_| {});

        // Assert
        assert_eq!(empty, Err(TextVectorError::Shape));
        assert_eq!(too_large, Err(TextVectorError::Shape));
    }
}
