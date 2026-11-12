//! PostgreSQL record text input and output (`record_in` / `record_out`).
use super::{CompositeRef, RecordValue};
use crate::types::extended::temporal::TemporalSession;
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

/// Rejected record text, with PostgreSQL's SQLSTATE and detail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordInputError {
    pub sqlstate: &'static str,
    pub message: String,
    pub detail: Option<String>,
}

impl RecordInputError {
    fn malformed(input: &str, detail: &str) -> Self {
        Self {
            sqlstate: "22P02",
            message: format!("malformed record literal: \"{input}\""),
            detail: Some(detail.to_string()),
        }
    }

    fn internal(message: String) -> Self {
        Self {
            sqlstate: "XX000",
            message,
            detail: None,
        }
    }
}

impl std::fmt::Display for RecordInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)?;
        if let Some(detail) = &self.detail {
            write!(formatter, ": {detail}")?;
        }
        Ok(())
    }
}

impl std::error::Error for RecordInputError {}

/// `record_out` using Cassie's default field renderers.
#[must_use]
pub fn format_record(record: &RecordValue, session: &TemporalSession) -> String {
    format_record_with(record, &mut |value, data_type| {
        default_field_text(value, data_type, session)
    })
}

/// `record_out` with a caller-supplied renderer for each non-NULL, non-record field.
pub fn format_record_with(
    record: &RecordValue,
    field_text: &mut dyn FnMut(&Value, &DataType) -> String,
) -> String {
    let mut out = String::from("(");
    for (index, (value, field)) in record
        .fields()
        .iter()
        .zip(&record.descriptor().fields)
        .enumerate()
    {
        if index > 0 {
            out.push(',');
        }
        if value.is_null() {
            continue;
        }
        let text = match value {
            Value::Typed(TypedValue::Record(nested)) => {
                format_record_with(nested, &mut *field_text)
            }
            other => field_text(other, &field.data_type),
        };
        push_record_field(&mut out, &text);
    }
    out.push(')');
    out
}

fn is_record_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// Quotes a field exactly where `record_out` does: empty text and text holding
/// a quote, backslash, parenthesis, comma or whitespace.
fn push_record_field(out: &mut String, text: &str) {
    let needs_quotes = text.is_empty()
        || text.chars().any(|character| {
            matches!(character, '"' | '\\' | '(' | ')' | ',') || is_record_space(character)
        });
    if !needs_quotes {
        out.push_str(text);
        return;
    }
    out.push('"');
    for character in text.chars() {
        if matches!(character, '"' | '\\') {
            out.push(character);
        }
        out.push(character);
    }
    out.push('"');
}

/// The text a field value renders as inside a record when no wire formatter
/// is supplied.
#[must_use]
pub fn default_field_text(
    value: &Value,
    data_type: &DataType,
    session: &TemporalSession,
) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => String::from(if *flag { "t" } else { "f" }),
        Value::Int64(number) => number.to_string(),
        Value::Float64(number) => crate::types::float_text::format(*number),
        Value::String(text) => text.clone(),
        Value::Vector(vector) => format!(
            "[{}]",
            vector
                .values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(document) if *data_type == DataType::Jsonb => {
            crate::types::jsonb::format(document).unwrap_or_else(|_| document.to_string())
        }
        Value::Json(document) => document.to_string(),
        Value::Array(array) => match data_type {
            DataType::Array(element) => {
                crate::types::array_value::format_text_in(array, element, session)
                    .unwrap_or_else(|| array.to_nested_json().to_string())
            }
            _ => array.to_nested_json().to_string(),
        },
        Value::Typed(typed) => typed.text_in(session),
    }
}

/// Splits `record_in` text into one optional raw field per column.
///
/// An entirely empty field is SQL NULL; a quoted empty field is the empty string.
///
/// # Errors
///
/// Returns PostgreSQL's `22P02` malformed record literal error.
pub fn split_record_text(
    text: &str,
    expected_fields: usize,
) -> Result<Vec<Option<String>>, RecordInputError> {
    let malformed = |detail: &str| RecordInputError::malformed(text, detail);
    let characters = text.chars().collect::<Vec<_>>();
    let mut position = 0;
    while characters
        .get(position)
        .copied()
        .is_some_and(is_record_space)
    {
        position += 1;
    }
    if characters.get(position) != Some(&'(') {
        return Err(malformed("Missing left parenthesis."));
    }
    position += 1;

    let mut fields = Vec::with_capacity(expected_fields);
    for index in 0..expected_fields {
        if matches!(characters.get(position), Some(',' | ')')) {
            fields.push(None);
        } else {
            let mut value = String::new();
            let mut in_quotes = false;
            loop {
                let Some(&character) = characters.get(position) else {
                    return Err(malformed("Unexpected end of input."));
                };
                if !in_quotes && matches!(character, ',' | ')') {
                    break;
                }
                position += 1;
                match character {
                    '\\' => {
                        let Some(&escaped) = characters.get(position) else {
                            return Err(malformed("Unexpected end of input."));
                        };
                        value.push(escaped);
                        position += 1;
                    }
                    '"' if !in_quotes => in_quotes = true,
                    '"' if characters.get(position) == Some(&'"') => {
                        value.push('"');
                        position += 1;
                    }
                    '"' => in_quotes = false,
                    other => value.push(other),
                }
            }
            fields.push(Some(value));
        }
        let (delimiter, detail) = if index + 1 < expected_fields {
            (',', "Too few columns.")
        } else {
            (')', "Too many columns.")
        };
        if characters.get(position) != Some(&delimiter) {
            return Err(malformed(detail));
        }
        position += 1;
    }
    if expected_fields == 0 {
        if characters.get(position) != Some(&')') {
            return Err(malformed("Too many columns."));
        }
        position += 1;
    }
    while characters
        .get(position)
        .copied()
        .is_some_and(is_record_space)
    {
        position += 1;
    }
    if position < characters.len() {
        return Err(malformed("Junk after right parenthesis."));
    }
    Ok(fields)
}

/// `record_in`: parses `text` as a value of `descriptor`, converting each
/// non-NULL field with `parse_field`.
///
/// # Errors
///
/// Returns the malformed-literal error or the first field conversion error.
pub fn parse_record_text<E: From<RecordInputError>>(
    text: &str,
    descriptor: &CompositeRef,
    mut parse_field: impl FnMut(&DataType, &str) -> Result<Value, E>,
) -> Result<RecordValue, E> {
    let parts = split_record_text(text, descriptor.fields.len())?;
    let mut values = Vec::with_capacity(parts.len());
    for (field, part) in descriptor.fields.iter().zip(parts) {
        values.push(match part {
            None => Value::Null,
            Some(raw) => parse_field(&field.data_type, &raw)?,
        });
    }
    RecordValue::new(descriptor.clone(), values)
        .map_err(|message| E::from(RecordInputError::internal(message)))
}
