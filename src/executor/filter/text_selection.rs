//! Character-indexed text selection without materializing character directories.

use super::{require_arg_count_range, signed_integer_arg, QueryError, Value};
use crate::app::CassieError;

pub(super) fn literal_argument(
    name: &str,
    index: usize,
    argument: &super::Expr,
    arguments: &[super::Expr],
    params: &[Value],
) -> Option<Result<Value, QueryError>> {
    let name = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let integer_position = match name {
        "substr" => index > 0,
        "left" | "right" => index == 1,
        "chr" => index == 0,
        "substring" => {
            index > 0
                && arguments.iter().skip(1).any(|expression| match expression {
                    super::Expr::IntegerLiteral(_) => true,
                    super::Expr::Cast { data_type, .. } => matches!(
                        data_type,
                        crate::types::DataType::SmallInt | crate::types::DataType::Int
                    ),
                    super::Expr::Param(slot) => matches!(params.get(*slot), Some(Value::Int64(_))),
                    _ => false,
                })
        }
        "split_part" => index == 2,
        _ => false,
    };
    let super::Expr::StringLiteral(input) = argument else {
        return None;
    };
    integer_position.then(|| {
        input
            .trim()
            .parse::<i32>()
            .map(|value| Value::Int64(i64::from(value)))
            .map_err(|error| {
                let message = if matches!(
                    error.kind(),
                    std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
                ) {
                    "integer out of range".to_owned()
                } else {
                    format!("invalid input syntax for type integer: {input:?}")
                };
                CassieError::Execution(message).into()
            })
    })
}

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let range = match name {
        "substring" | "substr" => 2..=3,
        "trim" | "btrim" | "ltrim" | "rtrim" => 1..=2,
        "split_part" => 3..=3,
        "strpos" | "position" => 2..=2,
        _ => return None,
    };
    Some((|| {
        require_arg_count_range(name, args, range)?;
        if name == "substring" && matches!(args.get(1), Some(Value::String(_))) {
            return Err(CassieError::Unsupported(
                "regexp SUBSTRING remains pending under #803".into(),
            )
            .into());
        }
        // These PostgreSQL builtins are strict, including error-producing inputs.
        if args.iter().any(|value| matches!(value, Value::Null)) {
            return Ok(Value::Null);
        }
        let text = text(name, &args[0])?;
        match name {
            "substring" | "substr" => substring(name, text, args),
            "trim" | "btrim" | "ltrim" | "rtrim" => {
                let characters = args
                    .get(1)
                    .map_or(Ok(" "), |value| self::text(name, value))?;
                let contains = |character| characters.contains(character);
                let selected = match name {
                    "ltrim" => text.trim_start_matches(contains),
                    "rtrim" => text.trim_end_matches(contains),
                    _ => text.trim_matches(contains),
                };
                Ok(Value::String(selected.to_owned()))
            }
            "split_part" => split_part(name, text, args),
            _ => {
                let needle = self::text(name, &args[1])?;
                let position = text
                    .find(needle)
                    .map_or(0, |offset| text[..offset].chars().count() + 1);
                Ok(Value::Int64(i64::from(i32::try_from(position).map_err(
                    |_| CassieError::Execution("integer out of range".into()),
                )?)))
            }
        }
    })())
}

pub(super) fn text<'a>(name: &str, value: &'a Value) -> Result<&'a str, QueryError> {
    match value {
        Value::String(text) => Ok(text),
        _ => Err(QueryError::General(format!(
            "function '{name}' expects text input"
        ))),
    }
}

fn substring(name: &str, text: &str, args: &[Value]) -> Result<Value, QueryError> {
    let start = i128::from(signed_integer_arg(name, &args[1])?.expect("strict argument"));
    let offset = (start - 1).max(0);
    let length = if let Some(value) = args.get(2) {
        let length = signed_integer_arg(name, value)?.expect("strict argument");
        if length < 0 {
            return Err(
                CassieError::Execution("negative substring length not allowed".into()).into(),
            );
        }
        usize::try_from((start + i128::from(length) - 1 - offset).max(0)).unwrap_or(usize::MAX)
    } else {
        usize::MAX
    };
    let offset = usize::try_from(offset).unwrap_or(usize::MAX);
    // Slice the original UTF-8 backing before copying the bounded result.
    let begin = text
        .char_indices()
        .nth(offset)
        .map_or(text.len(), |(index, _)| index);
    let end = text[begin..]
        .char_indices()
        .nth(length)
        .map_or(text.len(), |(index, _)| begin + index);
    Ok(Value::String(text[begin..end].to_owned()))
}

fn split_part(name: &str, text: &str, args: &[Value]) -> Result<Value, QueryError> {
    let delimiter = self::text(name, &args[1])?;
    let index = signed_integer_arg(name, &args[2])?.expect("strict argument");
    if index == 0 {
        return Err(
            CassieError::InvalidParameterValue("field position must not be zero".into()).into(),
        );
    }
    if delimiter.is_empty() {
        return Ok(Value::String(
            if matches!(index, -1 | 1) { text } else { "" }.to_owned(),
        ));
    }
    let ordinal = if index > 0 {
        i128::from(index) - 1
    } else {
        i128::try_from(text.split(delimiter).count()).unwrap_or(i128::MAX) + i128::from(index)
    };
    let selected = usize::try_from(ordinal)
        .ok()
        .and_then(|index| text.split(delimiter).nth(index))
        .unwrap_or("");
    Ok(Value::String(selected.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{evaluate, Value};

    #[test]
    fn should_preserve_signed_character_ranges_given_unicode_and_extreme_positions_when_substrings_are_selected(
    ) {
        // Arrange
        let cases = [
            (0, 4, "Aé🙂"),
            (-2, 4, "A"),
            (2, 2, "é🙂"),
            (i64::MIN, i64::MAX, ""),
            (i64::MAX, i64::MAX, ""),
        ];

        // Act
        for (start, length, expected) in cases {
            let selected = evaluate(
                "substring",
                &[
                    Value::String("Aé🙂Z".into()),
                    Value::Int64(start),
                    Value::Int64(length),
                ],
            )
            .expect("substring builtin");

            // Assert
            assert_eq!(
                selected.expect("bounded signed range"),
                Value::String(expected.into())
            );
        }
    }

    #[test]
    fn should_preserve_scalar_selection_values_given_unicode_delimiters_and_nulls_when_builtins_execute(
    ) {
        // Arrange
        let cases = [
            (
                "trim",
                vec![Value::String(" \t x \n ".into())],
                Value::String("\t x \n".into()),
            ),
            (
                "split_part",
                vec![
                    Value::String("abc".into()),
                    Value::String(String::new()),
                    Value::Int64(-1),
                ],
                Value::String("abc".into()),
            ),
            (
                "split_part",
                vec![
                    Value::String("a,,b,".into()),
                    Value::String(",".into()),
                    Value::Int64(-1),
                ],
                Value::String(String::new()),
            ),
            (
                "split_part",
                vec![
                    Value::String("a,b".into()),
                    Value::String(",".into()),
                    Value::Int64(i64::MIN),
                ],
                Value::String(String::new()),
            ),
            (
                "substring",
                vec![Value::Null, Value::Int64(1), Value::Int64(-1)],
                Value::Null,
            ),
            (
                "strpos",
                vec![Value::String("Aé🙂Z".into()), Value::String("🙂".into())],
                Value::Int64(3),
            ),
        ];

        // Act
        for (name, args, expected) in cases {
            let selected = evaluate(name, &args).expect("text selection builtin");

            // Assert
            assert_eq!(selected.expect("valid text inputs"), expected, "{name}");
        }
    }
}
