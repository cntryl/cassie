//! Bounded scalar text transformation functions.

use super::{require_arg_count_range, signed_integer_arg, text_selection, QueryError, Value};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::collections::HashMap;

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    let range = match name {
        "overlay" => 3..=4,
        "lpad" | "rpad" => 2..=3,
        "repeat" => 2..=2,
        "translate" => 3..=3,
        "reverse" | "initcap" => 1..=1,
        "concat_ws" => 1..=usize::MAX,
        _ => return None,
    };
    Some((|| {
        require_arg_count_range(name, args, range)?;
        if name == "concat_ws" {
            return concat_ws(args, controls);
        }
        if args.iter().any(|value| matches!(value, Value::Null)) {
            return Ok(Value::Null);
        }
        match name {
            "overlay" => overlay(args, controls),
            "lpad" | "rpad" => pad(name, args, controls),
            "repeat" => repeat(args, controls),
            "reverse" => reverse(args, controls),
            "initcap" => initcap(args, controls),
            "translate" => translate(args, controls),
            _ => unreachable!("text transform dispatch"),
        }
    })())
}

fn overlay(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    let source = text_selection::text("overlay", &args[0])?;
    let placing = text_selection::text("overlay", &args[1])?;
    let start = signed_integer_arg("overlay", &args[2])?.expect("strict argument");
    if start < 1 {
        return Err(CassieError::Execution("negative substring length not allowed".into()).into());
    }
    let start = usize::try_from(start - 1).unwrap_or(usize::MAX);
    let remove = args
        .get(3)
        .map(|value| signed_integer_arg("overlay", value))
        .transpose()?
        .flatten()
        .unwrap_or_else(|| i64::try_from(placing.chars().count()).unwrap_or(i64::MAX));
    let end = if remove < 0 {
        start.saturating_sub(usize::try_from(remove.unsigned_abs()).unwrap_or(usize::MAX))
    } else {
        start.saturating_add(usize::try_from(remove).unwrap_or(usize::MAX))
    };
    let prefix_end = character_boundary(source, start);
    let suffix_start = character_boundary(source, end);
    let output_len = source[..prefix_end]
        .len()
        .checked_add(placing.len())
        .and_then(|length| length.checked_add(source[suffix_start..].len()))
        .ok_or_else(|| resource_limit("overlay output size overflow"))?;
    let _memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, "overlay")?;
    output.push_str(&source[..prefix_end]);
    output.push_str(placing);
    output.push_str(&source[suffix_start..]);
    Ok(Value::String(output))
}

fn pad(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let source = text_selection::text(name, &args[0])?;
    let target = signed_integer_arg(name, &args[1])?.expect("strict argument");
    if target <= 0 {
        return Ok(Value::String(String::new()));
    }
    let target = usize::try_from(target).unwrap_or(usize::MAX);
    let source_chars = source.chars().count();
    let kept_chars = target.min(source_chars);
    let source_end = character_boundary(source, kept_chars);
    if target <= source_chars {
        let _memory = reserve_output(controls, source_end)?;
        return Ok(Value::String(source[..source_end].to_owned()));
    }
    let pattern = args
        .get(2)
        .map_or(Ok(" "), |value| text_selection::text(name, value))?;
    let pad_chars = target - source_chars;
    let pad_bytes = repeated_character_bytes(pattern, pad_chars)
        .ok_or_else(|| resource_limit("padding output size overflow"))?;
    let output_len = source_end
        .checked_add(pad_bytes)
        .ok_or_else(|| resource_limit("padding output size overflow"))?;
    let _memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, name)?;
    if name == "lpad" {
        append_repeated_characters(&mut output, pattern, pad_chars, controls)?;
        output.push_str(&source[..source_end]);
    } else {
        output.push_str(&source[..source_end]);
        append_repeated_characters(&mut output, pattern, pad_chars, controls)?;
    }
    Ok(Value::String(output))
}

fn repeat(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    let source = text_selection::text("repeat", &args[0])?;
    let count = signed_integer_arg("repeat", &args[1])?.expect("strict argument");
    if count <= 0 || source.is_empty() {
        return Ok(Value::String(String::new()));
    }
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    let output_len = source
        .len()
        .checked_mul(count)
        .ok_or_else(|| resource_limit("repeat output size overflow"))?;
    let _memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, "repeat")?;
    for index in 0..count {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        output.push_str(source);
    }
    Ok(Value::String(output))
}

fn reverse(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    let source = text_selection::text("reverse", &args[0])?;
    let _memory = reserve_output(controls, source.len())?;
    let mut output = output_string(source.len(), "reverse")?;
    for (index, character) in source.chars().rev().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        output.push(character);
    }
    Ok(Value::String(output))
}

fn initcap(args: &[Value], controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    let source = text_selection::text("initcap", &args[0])?;
    let mut output_len = 0_usize;
    let mut at_word_start = true;
    for (index, character) in source.chars().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        if !character.is_alphanumeric() {
            at_word_start = true;
            output_len = output_len
                .checked_add(character.len_utf8())
                .ok_or_else(|| resource_limit("initcap output size overflow"))?;
            continue;
        }
        let mapped_len = if at_word_start {
            character.to_uppercase().map(char::len_utf8).sum::<usize>()
        } else {
            character.to_lowercase().map(char::len_utf8).sum::<usize>()
        };
        at_word_start = false;
        output_len = output_len
            .checked_add(mapped_len)
            .ok_or_else(|| resource_limit("initcap output size overflow"))?;
    }
    let _memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, "initcap")?;
    at_word_start = true;
    for (index, character) in source.chars().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        if character.is_alphanumeric() {
            if at_word_start {
                output.extend(character.to_uppercase());
            } else {
                output.extend(character.to_lowercase());
            }
            at_word_start = false;
        } else {
            output.push(character);
            at_word_start = true;
        }
    }
    Ok(Value::String(output))
}

fn translate(
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let source = text_selection::text("translate", &args[0])?;
    let from = text_selection::text("translate", &args[1])?;
    let to = text_selection::text("translate", &args[2])?;
    let from_chars = from.chars().count();
    let scratch_bytes = from_chars
        .checked_mul(32)
        .ok_or_else(|| resource_limit("translate mapping size overflow"))?;
    let _scratch_memory = reserve_output(controls, scratch_bytes)?;
    let mut mapping = HashMap::<char, Option<char>>::new();
    mapping
        .try_reserve(from_chars)
        .map_err(|error| allocation_error("translate mapping", &error))?;
    let mut replacements = to.chars();
    for (index, character) in from.chars().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        let replacement = replacements.next();
        mapping.entry(character).or_insert(replacement);
    }
    let mut output_len = 0_usize;
    for (index, character) in source.chars().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        let bytes = match mapping.get(&character) {
            Some(Some(replacement)) => replacement.len_utf8(),
            Some(None) => 0,
            None => character.len_utf8(),
        };
        output_len = output_len
            .checked_add(bytes)
            .ok_or_else(|| resource_limit("translate output size overflow"))?;
    }
    let _output_memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, "translate")?;
    for (index, character) in source.chars().enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        match mapping.get(&character) {
            Some(Some(replacement)) => output.push(*replacement),
            Some(None) => {}
            None => output.push(character),
        }
    }
    Ok(Value::String(output))
}

fn concat_ws(
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let Value::String(separator) = &args[0] else {
        return if matches!(&args[0], Value::Null) {
            Ok(Value::Null)
        } else {
            Err(QueryError::General(
                "function 'concat_ws' expects text input".into(),
            ))
        };
    };
    let mut output_len = 0_usize;
    let mut selected = 0_usize;
    for (index, value) in args.iter().skip(1).enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        let Value::String(text) = value else {
            if matches!(value, Value::Null) {
                continue;
            }
            return Err(QueryError::General(
                "function 'concat_ws' expects text input".into(),
            ));
        };
        if selected > 0 {
            output_len = output_len
                .checked_add(separator.len())
                .ok_or_else(|| resource_limit("concat_ws output size overflow"))?;
        }
        output_len = output_len
            .checked_add(text.len())
            .ok_or_else(|| resource_limit("concat_ws output size overflow"))?;
        selected += 1;
    }
    let _memory = reserve_output(controls, output_len)?;
    let mut output = output_string(output_len, "concat_ws")?;
    selected = 0;
    for (index, value) in args.iter().skip(1).enumerate() {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        let Value::String(text) = value else {
            continue;
        };
        if selected > 0 {
            output.push_str(separator);
        }
        output.push_str(text);
        selected += 1;
    }
    Ok(Value::String(output))
}

fn repeated_character_bytes(pattern: &str, characters: usize) -> Option<usize> {
    if characters == 0 || pattern.is_empty() {
        return Some(0);
    }
    let pattern_characters = pattern.chars().count();
    let full_patterns = characters / pattern_characters;
    let trailing_characters = characters % pattern_characters;
    let trailing_bytes = character_boundary(pattern, trailing_characters);
    full_patterns
        .checked_mul(pattern.len())?
        .checked_add(trailing_bytes)
}

fn append_repeated_characters(
    output: &mut String,
    pattern: &str,
    characters: usize,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    if characters == 0 || pattern.is_empty() {
        return Ok(());
    }
    let pattern_characters = pattern.chars().count();
    let full_patterns = characters / pattern_characters;
    for index in 0..full_patterns {
        if index % 1024 == 0 {
            check_interruption(controls)?;
        }
        output.push_str(pattern);
    }
    let trailing_characters = characters % pattern_characters;
    let trailing_bytes = character_boundary(pattern, trailing_characters);
    output.push_str(&pattern[..trailing_bytes]);
    Ok(())
}

fn character_boundary(text: &str, characters: usize) -> usize {
    text.char_indices()
        .nth(characters)
        .map_or(text.len(), |(offset, _)| offset)
}

fn reserve_output(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    let memory = controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()?;
    check_interruption(controls)?;
    Ok(memory)
}

fn output_string(capacity: usize, function: &str) -> Result<String, QueryError> {
    let mut output = String::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|error| allocation_error(function, &error))?;
    Ok(output)
}

fn allocation_error(function: &str, error: &std::collections::TryReserveError) -> QueryError {
    CassieError::ResourceLimit(format!("unable to allocate {function} output: {error}")).into()
}

fn resource_limit(message: &str) -> QueryError {
    CassieError::ResourceLimit(message.to_owned()).into()
}

fn check_interruption(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{evaluate, QueryError, Value};
    use crate::app::CassieError;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    fn controls_with_memory_budget(bytes: usize) -> QueryExecutionControls {
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..crate::config::CassieRuntimeLimits::default()
        };
        QueryExecutionControls::from_limits(&limits, std::time::Instant::now())
    }

    #[test]
    fn should_admit_bounded_repeated_text_given_a_small_count() {
        // Arrange
        let controls = controls_with_memory_budget(16);
        let args = [Value::String("ab".into()), Value::Int64(3)];

        // Act
        let result = evaluate("repeat", &args, Some(&controls));

        // Assert
        assert_eq!(
            result.expect("repeat builtin").expect("bounded output"),
            Value::String("ababab".into())
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reject_repeated_text_before_allocation_when_query_memory_is_exceeded() {
        // Arrange
        let controls = controls_with_memory_budget(8);
        let args = [Value::String("ab".into()), Value::Int64(5)];

        // Act
        let result = evaluate("repeat", &args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Some(Err(QueryError::Cassie(CassieError::ResourceLimit(_))))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_release_output_admission_when_text_transform_is_cancelled() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();
        let args = [Value::String("ab".into()), Value::Int64(5)];

        // Act
        let result = evaluate("repeat", &args, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Some(Err(QueryError::Cassie(CassieError::QueryCancelled)))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
