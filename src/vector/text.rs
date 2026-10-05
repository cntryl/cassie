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
pub(crate) fn visit_components(
    text: &str,
    dimensions: usize,
    component: impl FnMut(f32),
) -> Result<(), TextVectorError> {
    let mut range_error = false;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer
        .deserialize_seq(TextVectorVisitor {
            dimensions,
            component,
            range_error: &mut range_error,
        })
        .map_err(|_| TextVectorError::Shape)?;
    deserializer.end().map_err(|_| TextVectorError::Shape)?;
    if range_error {
        Err(TextVectorError::Range)
    } else {
        Ok(())
    }
}

struct TextVectorVisitor<'a, F> {
    dimensions: usize,
    component: F,
    range_error: &'a mut bool,
}

impl<'de, F: FnMut(f32)> Visitor<'de> for TextVectorVisitor<'_, F> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "a finite vector of {} components",
            self.dimensions
        )
    }

    fn visit_seq<A>(mut self, mut sequence: A) -> Result<(), A::Error>
    where
        A: SeqAccess<'de>,
    {
        for _ in 0..self.dimensions {
            let token = sequence
                .next_element::<&'de RawValue>()?
                .ok_or_else(|| <A::Error as de::Error>::custom("vector dimension mismatch"))?;
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
        // Read at most one excess token. No JSON array or numeric Vec is built,
        // and wrong width is rejected before any caller output construction.
        if sequence.next_element::<&'de RawValue>()?.is_some() {
            return Err(<A::Error as de::Error>::custom("vector dimension mismatch"));
        }
        Ok(())
    }
}
