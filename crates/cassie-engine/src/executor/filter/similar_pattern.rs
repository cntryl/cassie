//! Controlled PostgreSQL 18 lexical SIMILAR-pattern translation.

use crate::{
    app::{CassieError, DiagnosticError},
    runtime::QueryExecutionControls,
    types::Value,
};

use super::pattern_memory::PatternText;
use super::{check_interruption, resource_limit, text_argument, QueryError};

pub(super) fn evaluate(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    evaluate_owned(arguments, controls).map(|text| match text {
        Some(text) => Value::String(text.into_parts().0),
        None => Value::Null,
    })
}

pub(super) fn evaluate_owned(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<PatternText>, QueryError> {
    if arguments.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(None);
    }
    let pattern = text_argument("similar_to_escape", arguments, 0)?;
    let escape = if arguments.len() == 1 {
        "\\"
    } else {
        text_argument("similar_to_escape", arguments, 1)?
    };
    let mut characters = escape.chars();
    let escape = characters.next();
    if characters.next().is_some() {
        return Err(CassieError::Execution("invalid escape string".into()).into());
    }
    let mut length = 0_usize;
    translate(pattern, escape, controls, |text| {
        length = length
            .checked_add(text.len())
            .ok_or_else(|| resource_limit("SIMILAR pattern output size overflow"))?;
        Ok(())
    })?;
    PatternText::build(length, controls, |writer| {
        translate(pattern, escape, controls, |text| writer.push_str(text))
    })
    .map(Some)
}

fn translate(
    pattern: &str,
    escape: Option<char>,
    controls: Option<&QueryExecutionControls>,
    mut emit: impl FnMut(&str) -> Result<(), QueryError>,
) -> Result<(), QueryError> {
    let mut after_escape = false;
    let mut quotes = 0_u8;
    let mut brackets = 0_usize;
    let mut class_position = 0_usize;
    let multibyte_escape = escape.is_some_and(|character| character.len_utf8() > 1);
    emit("^(?:")?;
    for (index, character) in pattern.chars().enumerate() {
        if index.is_multiple_of(1024) {
            check_interruption(controls)?;
        }
        let mut bytes = [0; 4];
        let text = character.encode_utf8(&mut bytes);
        // Preserve the oracle's multibyte escape path, including its lexical
        // bracket-position behavior. This helper translates without compiling.
        if multibyte_escape && character.len_utf8() > 1 {
            if after_escape {
                emit("\\")?;
                emit(text)?;
                after_escape = false;
            } else if Some(character) == escape {
                after_escape = true;
            } else {
                emit(text)?;
            }
            continue;
        }
        if after_escape {
            if character == '"' && brackets == 0 {
                match quotes {
                    0 => emit("){1,1}?(")?,
                    1 => emit("){1,1}(?:")?,
                    _ => return Err(CassieError::Diagnostic(Box::new(DiagnosticError::new(
                        "2200C", "SQL regular expression may not contain more than two escape-double-quote separators",
                    ))).into()),
                }
                quotes += 1;
            } else {
                emit("\\")?;
                emit(text)?;
                class_position = 3;
            }
            after_escape = false;
        } else if Some(character) == escape {
            after_escape = true;
        } else if brackets > 0 {
            if character == '\\' {
                emit("\\")?;
            }
            emit(text)?;
            match character {
                ']' if class_position > 2 => brackets -= 1,
                '[' => {
                    brackets += 1;
                    class_position = 3;
                }
                '^' => class_position += 1,
                _ => class_position = 3,
            }
        } else {
            match character {
                '[' => {
                    emit(text)?;
                    brackets = 1;
                    class_position = 1;
                }
                '%' => emit(".*")?,
                '_' => emit(".")?,
                '(' => emit("(?:")?,
                '\\' | '.' | '^' | '$' => {
                    emit("\\")?;
                    emit(text)?;
                }
                _ => emit(text)?,
            }
        }
    }
    emit(")$")?;
    check_interruption(controls)
}

#[cfg(test)]
mod tests {
    use super::evaluate;
    use crate::{
        app::CassieError,
        config::CassieRuntimeLimits,
        executor::filter::QueryError,
        runtime::{QueryCancellationHandle, QueryExecutionControls},
        types::Value,
    };

    #[test]
    fn should_preserve_oracle_similar_bracket_escape_translations() {
        // Arrange
        let cases = [
            ("[^]_a]", "\\", "^(?:[^]_a])$"),
            ("[!]]_%", "!", "^(?:[\\]]..*)$"),
            ("[\\\"]_%", "\\", "^(?:[\\\"]..*)$"),
            ("[[:alpha:]_%]_%", "\\", "^(?:[[:alpha:]_%]..*)$"),
            ("[\\]_%", "", "^(?:[\\\\]..*)$"),
            ("[💡é]_%", "💡", "^(?:[\\é]_%)$"),
            ("[é]_%", "💡", "^(?:[é]_%)$"),
            ("a💡\"b💡\"c", "💡", "^(?:a){1,1}?(b){1,1}(?:c)$"),
            ("x💡", "💡", "^(?:x)$"),
            ("x!!%", "!", "^(?:x\\!.*)$"),
            ("[]_%]_%", "\\", "^(?:[]_%]..*)$"),
            ("a\\b", "", "^(?:a\\\\b)$"),
        ];

        // Act
        let observations = cases.map(|(pattern, escape, _)| {
            evaluate(
                &[Value::String(pattern.into()), Value::String(escape.into())],
                None,
            )
        });

        // Assert
        for ((pattern, escape, expected), observation) in cases.into_iter().zip(observations) {
            assert_eq!(
                observation.expect("lexical helper does not compile regexes"),
                Value::String(expected.into()),
                "pattern={pattern:?}, escape={escape:?}"
            );
        }
    }

    #[test]
    fn should_admit_similar_translation_at_exact_output_bytes() {
        // Arrange
        let arguments = [Value::String("a%b".into())];
        let limits = |bytes| CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        };
        let below = QueryExecutionControls::from_limits(&limits(9), std::time::Instant::now());
        let exact = QueryExecutionControls::from_limits(&limits(10), std::time::Instant::now());

        // Act
        let rejected = evaluate(&arguments, Some(&below));
        let accepted = evaluate(&arguments, Some(&exact));

        // Assert
        assert!(matches!(
            rejected,
            Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(
            accepted.expect("exact output admission"),
            Value::String("^(?:a.*b)$".into())
        );
        assert_eq!(below.current_query_memory_bytes(), 0);
        assert_eq!(exact.current_query_memory_bytes(), 0);
        assert_eq!(exact.peak_query_memory_bytes(), 10);
    }

    #[test]
    fn should_stop_similar_translation_after_a_scanned_token_is_canceled() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        let mut scanned = false;

        // Act
        let result = super::translate("abc", Some('\\'), Some(&controls), |text| {
            if text == "a" {
                scanned = true;
                cancellation.cancel();
            }
            Ok(())
        });

        // Assert
        assert!(scanned);
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCanceled))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
    }

    #[test]
    fn should_stop_similar_translation_before_output_allocation_when_canceled() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();
        let arguments = [Value::String("a%b".into())];

        // Act
        let result = evaluate(&arguments, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(CassieError::QueryCanceled))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
    }
}
