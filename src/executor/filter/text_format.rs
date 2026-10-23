//! Bounded PostgreSQL text replacement and formatting.

use std::borrow::Cow;

use super::{QueryError, Value};
use crate::app::{CassieError, DiagnosticError};
use crate::runtime::QueryExecutionControls;

enum FormatSegment<'a> {
    Literal(&'a str),
    Percent,
    Field {
        value: &'a Value,
        specifier: char,
        width: usize,
        left_justified: bool,
    },
}

struct FormatParser<'a> {
    format: &'a str,
    args: &'a [Value],
    controls: Option<&'a QueryExecutionControls>,
    offset: usize,
    next_argument: usize,
}

impl<'a> FormatParser<'a> {
    fn new(
        format: &'a str,
        args: &'a [Value],
        controls: Option<&'a QueryExecutionControls>,
    ) -> Self {
        Self {
            format,
            args,
            controls,
            offset: 0,
            next_argument: 0,
        }
    }

    fn next(&mut self) -> Result<Option<FormatSegment<'a>>, QueryError> {
        check_interruption(self.controls)?;
        if self.offset >= self.format.len() {
            return Ok(None);
        }
        let remaining = &self.format[self.offset..];
        let Some(percent_offset) = remaining.find('%') else {
            self.offset = self.format.len();
            return Ok(Some(FormatSegment::Literal(remaining)));
        };
        if percent_offset > 0 {
            self.offset += percent_offset;
            return Ok(Some(FormatSegment::Literal(&remaining[..percent_offset])));
        }

        self.offset += 1;
        if self.format.as_bytes().get(self.offset) == Some(&b'%') {
            self.offset += 1;
            return Ok(Some(FormatSegment::Percent));
        }

        let bytes = self.format.as_bytes();
        let value_position = parse_position(bytes, &mut self.offset)?;
        let mut left_justified = false;
        if bytes.get(self.offset) == Some(&b'-') {
            left_justified = true;
            self.offset += 1;
        }

        let mut width = 0;
        let mut dynamic_width_position = None;
        let mut has_dynamic_width = false;
        if bytes.get(self.offset) == Some(&b'*') {
            has_dynamic_width = true;
            self.offset += 1;
            dynamic_width_position = parse_position(bytes, &mut self.offset)?;
        } else if bytes.get(self.offset).is_some_and(u8::is_ascii_digit) {
            width = parse_width(bytes, &mut self.offset)?;
        }

        let Some(specifier) = self
            .format
            .get(self.offset..)
            .and_then(|suffix| suffix.chars().next())
        else {
            return Err(unterminated_format_specifier());
        };
        self.offset += specifier.len_utf8();
        if !matches!(specifier, 's' | 'I' | 'L') {
            return Err(invalid_format_specifier(&specifier.to_string()));
        }

        if has_dynamic_width {
            let width_value = self.take_argument(dynamic_width_position)?;
            let width_value = integer_width(width_value)?;
            if width_value < 0 {
                left_justified = true;
                width = usize::try_from(width_value.unsigned_abs()).map_err(|_| {
                    resource_limit("format() output width exceeds the addressable range")
                })?;
            } else {
                width = usize::try_from(width_value).map_err(|_| {
                    resource_limit("format() output width exceeds the addressable range")
                })?;
            }
        }

        let value = self.take_argument(value_position)?;
        Ok(Some(FormatSegment::Field {
            value,
            specifier,
            width,
            left_justified,
        }))
    }

    fn take_argument(&mut self, position: Option<usize>) -> Result<&'a Value, QueryError> {
        let index = if let Some(position) = position {
            let index = position
                .checked_sub(1)
                .ok_or_else(zero_format_argument_position)?;
            self.next_argument = position;
            index
        } else {
            let index = self.next_argument;
            self.next_argument = self.next_argument.checked_add(1).ok_or_else(|| {
                resource_limit("format() argument position exceeds the addressable range")
            })?;
            index
        };
        self.args
            .get(index)
            .ok_or_else(|| invalid_parameter("too few arguments for format()"))
    }
}

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    match name {
        "format" => Some(format(args, controls)),
        "replace" => Some(replace(args, controls)),
        _ => None,
    }
}

fn format(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    check_interruption(controls)?;
    let Some(format_value) = args.first() else {
        return Err(invalid_parameter("format() requires a format string"));
    };
    if matches!(format_value, Value::Null) {
        return Ok(Value::Null);
    }
    let format_string = super::text_selection::text("format", format_value)?;
    let format_args = &args[1..];
    let output_length = formatted_length(format_string, format_args, controls)?;
    let _memory = reserve_output(controls, output_length)?;
    let mut output = String::new();
    output
        .try_reserve_exact(output_length)
        .map_err(|error| resource_limit(&format!("unable to allocate format() output: {error}")))?;
    render(format_string, format_args, &mut output, controls)?;
    Ok(Value::String(output))
}

fn formatted_length(
    format: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, QueryError> {
    let mut parser = FormatParser::new(format, args, controls);
    let mut length = 0usize;
    while let Some(segment) = parser.next()? {
        let segment_length = match segment {
            FormatSegment::Literal(value) => value.len(),
            FormatSegment::Percent => 1,
            FormatSegment::Field {
                value,
                specifier,
                width,
                ..
            } => {
                let (byte_length, character_length) =
                    formatted_value_lengths(value, specifier, controls)?;
                byte_length
                    .checked_add(width.saturating_sub(character_length))
                    .ok_or_else(|| resource_limit("format() output size overflow"))?
            }
        };
        length = length
            .checked_add(segment_length)
            .ok_or_else(|| resource_limit("format() output size overflow"))?;
    }
    Ok(length)
}

fn formatted_value_lengths(
    value: &Value,
    specifier: char,
    controls: Option<&QueryExecutionControls>,
) -> Result<(usize, usize), QueryError> {
    match (specifier, value) {
        ('s', Value::Null) => Ok((0, 0)),
        ('L', Value::Null) => Ok((4, 4)),
        ('I', Value::Null) => Err(null_identifier()),
        _ => {
            let value = value_text(value);
            match specifier {
                's' => Ok((value.len(), character_metrics(&value, '\0', controls)?.0)),
                'I' => identifier_lengths(&value, controls),
                'L' => literal_lengths(&value, controls),
                _ => Err(invalid_format_specifier(&specifier.to_string())),
            }
        }
    }
}

fn render(
    format: &str,
    args: &[Value],
    output: &mut String,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let mut parser = FormatParser::new(format, args, controls);
    while let Some(segment) = parser.next()? {
        match segment {
            FormatSegment::Literal(value) => append_text(output, value, controls)?,
            FormatSegment::Percent => output.push('%'),
            FormatSegment::Field {
                value,
                specifier,
                width,
                left_justified,
            } => {
                let text = match (specifier, value) {
                    ('s', Value::Null) => Cow::Borrowed(""),
                    ('L', Value::Null) => Cow::Borrowed("NULL"),
                    ('I', Value::Null) => return Err(null_identifier()),
                    _ => value_text(value),
                };
                let (_, character_length) = formatted_value_lengths(value, specifier, controls)?;
                let padding = width.saturating_sub(character_length);
                if !left_justified {
                    push_spaces(output, padding, controls)?;
                }
                match (specifier, value) {
                    ('s', _) => append_text(output, &text, controls)?,
                    ('I', _) => append_identifier(output, &text, controls)?,
                    ('L', Value::Null) => output.push_str("NULL"),
                    ('L', _) => append_literal(output, &text, controls)?,
                    _ => return Err(invalid_format_specifier(&specifier.to_string())),
                }
                if left_justified {
                    push_spaces(output, padding, controls)?;
                }
            }
        }
    }
    Ok(())
}

fn value_text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(value) => Cow::Borrowed(value),
        _ => Cow::Owned(super::to_text(value)),
    }
}

fn identifier_lengths(
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(usize, usize), QueryError> {
    let (character_length, escaped_quotes) = character_metrics(value, '"', controls)?;
    if super::identifier_is_unquoted(value) {
        return Ok((value.len(), character_length));
    }
    let byte_length = value
        .len()
        .checked_add(2)
        .and_then(|length| length.checked_add(escaped_quotes))
        .ok_or_else(|| resource_limit("format() output size overflow"))?;
    let character_length = character_length
        .checked_add(2)
        .and_then(|length| length.checked_add(escaped_quotes))
        .ok_or_else(|| resource_limit("format() output size overflow"))?;
    Ok((byte_length, character_length))
}

fn character_metrics(
    value: &str,
    escaped: char,
    controls: Option<&QueryExecutionControls>,
) -> Result<(usize, usize), QueryError> {
    let mut character_length = 0usize;
    let mut escaped_count = 0usize;
    let mut next_check = 0;
    for (index, character) in value.chars().enumerate() {
        if index >= next_check {
            check_interruption(controls)?;
            next_check = index.saturating_add(4096);
        }
        character_length = character_length
            .checked_add(1)
            .ok_or_else(|| resource_limit("format() output size overflow"))?;
        if character == escaped {
            escaped_count = escaped_count
                .checked_add(1)
                .ok_or_else(|| resource_limit("format() output size overflow"))?;
        }
    }
    check_interruption(controls)?;
    Ok((character_length, escaped_count))
}

fn literal_lengths(
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(usize, usize), QueryError> {
    let (character_length, escaped_quotes) = character_metrics(value, '\'', controls)?;
    let byte_length = value
        .len()
        .checked_add(2)
        .and_then(|length| length.checked_add(escaped_quotes))
        .ok_or_else(|| resource_limit("format() output size overflow"))?;
    let character_length = character_length
        .checked_add(2)
        .and_then(|length| length.checked_add(escaped_quotes))
        .ok_or_else(|| resource_limit("format() output size overflow"))?;
    Ok((byte_length, character_length))
}

fn append_identifier(
    output: &mut String,
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    if super::identifier_is_unquoted(value) {
        return append_text(output, value, controls);
    }
    output.push('"');
    let mut next_check = 0;
    for (index, character) in value.chars().enumerate() {
        if index >= next_check {
            check_interruption(controls)?;
            next_check = index.saturating_add(4096);
        }
        output.push(character);
        if character == '"' {
            output.push('"');
        }
    }
    output.push('"');
    Ok(())
}

fn append_literal(
    output: &mut String,
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    output.push('\'');
    let mut next_check = 0;
    for (index, character) in value.chars().enumerate() {
        if index >= next_check {
            check_interruption(controls)?;
            next_check = index.saturating_add(4096);
        }
        output.push(character);
        if character == '\'' {
            output.push('\'');
        }
    }
    output.push('\'');
    Ok(())
}

fn append_text(
    output: &mut String,
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let mut start = 0;
    let mut next_check = 0;
    for (index, (offset, _)) in value.char_indices().enumerate() {
        if index >= next_check {
            check_interruption(controls)?;
            if index > 0 {
                output.push_str(&value[start..offset]);
                start = offset;
            }
            next_check = index.saturating_add(4096);
        }
    }
    check_interruption(controls)?;
    output.push_str(&value[start..]);
    Ok(())
}

fn push_spaces(
    output: &mut String,
    mut count: usize,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    while count > 0 {
        check_interruption(controls)?;
        let chunk = count.min(4096);
        output.extend(std::iter::repeat_n(' ', chunk));
        count -= chunk;
    }
    Ok(())
}

fn replace(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    check_interruption(controls)?;
    let [source, search, replacement] = args else {
        return Err(QueryError::General("replace requires 3 args".to_owned()));
    };
    if args.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let source = super::text_selection::text("replace", source)?;
    let search = super::text_selection::text("replace", search)?;
    let replacement = super::text_selection::text("replace", replacement)?;
    let occurrences = if search.is_empty() {
        0
    } else {
        let mut occurrences = 0usize;
        for _ in source.match_indices(search) {
            check_interruption(controls)?;
            occurrences = occurrences
                .checked_add(1)
                .ok_or_else(|| resource_limit("replace() output size overflow"))?;
        }
        occurrences
    };
    let output_length = if search.is_empty() {
        source.len()
    } else {
        let retained = occurrences
            .checked_mul(search.len())
            .ok_or_else(|| resource_limit("replace() output size overflow"))?;
        source
            .len()
            .checked_sub(retained)
            .and_then(|length| {
                occurrences
                    .checked_mul(replacement.len())
                    .and_then(|added| length.checked_add(added))
            })
            .ok_or_else(|| resource_limit("replace() output size overflow"))?
    };
    let _memory = reserve_output(controls, output_length)?;
    let mut output = String::new();
    output.try_reserve_exact(output_length).map_err(|error| {
        resource_limit(&format!("unable to allocate replace() output: {error}"))
    })?;
    if search.is_empty() {
        append_text(&mut output, source, controls)?;
    } else {
        let mut offset = 0;
        for (index, _) in source.match_indices(search) {
            check_interruption(controls)?;
            append_text(&mut output, &source[offset..index], controls)?;
            append_text(&mut output, replacement, controls)?;
            offset = index + search.len();
        }
        append_text(&mut output, &source[offset..], controls)?;
    }
    Ok(Value::String(output))
}

fn reserve_output(
    controls: Option<&QueryExecutionControls>,
    length: usize,
) -> Result<Option<crate::runtime::QueryMemoryReservation>, QueryError> {
    let reservation = controls
        .map(|controls| controls.reserve_query_memory(length))
        .transpose()?;
    check_interruption(controls)?;
    Ok(reservation)
}

fn check_interruption(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(())
}

fn parse_position(bytes: &[u8], offset: &mut usize) -> Result<Option<usize>, QueryError> {
    let start = *offset;
    let Some(position) = parse_digits(bytes, offset)? else {
        return Ok(None);
    };
    if bytes.get(*offset) != Some(&b'$') {
        *offset = start;
        return Ok(None);
    }
    *offset += 1;
    if position == 0 {
        return Err(zero_format_argument_position());
    }
    Ok(Some(position))
}

fn parse_width(bytes: &[u8], offset: &mut usize) -> Result<usize, QueryError> {
    parse_digits(bytes, offset)?.ok_or_else(|| invalid_parameter("invalid format() width"))
}

fn parse_digits(bytes: &[u8], offset: &mut usize) -> Result<Option<usize>, QueryError> {
    let start = *offset;
    let mut value = 0usize;
    while let Some(byte) = bytes.get(*offset).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(usize::from(*byte - b'0')))
            .ok_or_else(|| resource_limit("format() width exceeds the addressable range"))?;
        *offset += 1;
    }
    Ok((*offset > start).then_some(value))
}

fn integer_width(value: &Value) -> Result<i64, QueryError> {
    if matches!(value, Value::Null) {
        return Ok(0);
    }
    let text = match value {
        Value::Bool(true) => "t".to_owned(),
        Value::Bool(false) => "f".to_owned(),
        _ => super::to_text(value),
    };
    crate::types::integer::parse(&text, crate::types::integer::IntegerWidth::I32).map_err(|error| {
        use crate::types::integer::IntegerInputError;

        let (sqlstate, message) = match error {
            IntegerInputError::Syntax => (
                "22P02",
                format!("invalid input syntax for type integer: \"{text}\""),
            ),
            IntegerInputError::Range => (
                "22003",
                format!("value \"{text}\" is out of range for type integer"),
            ),
        };
        QueryError::Cassie(CassieError::TypeInput(
            crate::types::extended::TypeInputError::new(sqlstate, message),
        ))
    })
}

fn zero_format_argument_position() -> QueryError {
    invalid_parameter("format specifies argument 0, but arguments are numbered from 1")
}

fn unterminated_format_specifier() -> QueryError {
    QueryError::Cassie(
        DiagnosticError::new("22023", "unterminated format() type specifier")
            .with_hint("For a single \"%\" use \"%%\".")
            .into(),
    )
}

fn invalid_format_specifier(specifier: &str) -> QueryError {
    QueryError::Cassie(
        DiagnosticError::new(
            "22023",
            format!("unrecognized format() type specifier \"{specifier}\""),
        )
        .with_hint("For a single \"%\" use \"%%\".")
        .into(),
    )
}

fn invalid_parameter(message: &str) -> QueryError {
    CassieError::InvalidParameterValue(message.to_owned()).into()
}

fn null_identifier() -> QueryError {
    QueryError::Cassie(
        DiagnosticError::new(
            "22004",
            "null values cannot be formatted as an SQL identifier",
        )
        .into(),
    )
}

fn resource_limit(message: &str) -> QueryError {
    CassieError::ResourceLimit(message.to_owned()).into()
}

#[cfg(test)]
mod tests {
    use super::{append_text, format, replace, reserve_output, QueryError, Value};
    use crate::app::CassieError;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    fn cancelled_controls() -> (QueryExecutionControls, QueryCancellationHandle) {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        (controls, cancellation)
    }

    fn controls_with_memory_budget(bytes: usize) -> QueryExecutionControls {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..crate::config::CassieRuntimeLimits::default()
        };
        QueryExecutionControls::from_limits(&limits, std::time::Instant::now())
    }

    #[test]
    fn should_stop_format_evaluation_when_query_is_cancelled() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();
        let args = [
            Value::String("%s".to_owned()),
            Value::String("value".to_owned()),
        ];

        // Act
        let result = format(&args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_stop_replace_evaluation_when_query_is_cancelled() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();
        let args = [
            Value::String("source".to_owned()),
            Value::String("source".to_owned()),
            Value::String("replacement".to_owned()),
        ];

        // Act
        let result = replace(&args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_admitted_output_when_cancellation_is_detected() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();

        // Act
        let result = reserve_output(Some(&controls), 8);

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 8);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_replace_reservation_when_cancellation_interrupts_copy() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        let reservation = reserve_output(Some(&controls), 8).expect("admitted replace output");
        cancellation.cancel();
        let mut output = String::new();

        // Act
        let result = append_text(&mut output, "replacement", Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCancelled))
        ));
        drop(reservation);
        assert_eq!(controls.peak_query_memory_bytes(), 8);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(output, "");
    }

    #[test]
    fn should_reject_format_output_when_query_memory_budget_is_exceeded() {
        // Arrange
        let controls = controls_with_memory_budget(16);
        let args = [
            Value::String("%100s".to_owned()),
            Value::String("x".to_owned()),
        ];

        // Act
        let result = format(&args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reject_replace_output_when_query_memory_budget_is_exceeded() {
        // Arrange
        let controls = controls_with_memory_budget(16);
        let args = [
            Value::String("a".repeat(32)),
            Value::String("a".to_owned()),
            Value::String("xx".to_owned()),
        ];

        // Act
        let result = replace(&args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
