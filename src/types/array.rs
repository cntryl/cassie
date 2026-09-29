use serde_json::Value as JsonValue;

use crate::types::DataType;

pub(crate) fn parse_text_array(input: &str, element_type: &DataType) -> Result<JsonValue, String> {
    let input = input.trim();
    if input.starts_with('[') {
        let value: JsonValue =
            serde_json::from_str(input).map_err(|error| format!("invalid JSON array: {error}"))?;
        return value
            .is_array()
            .then_some(value)
            .ok_or_else(|| "array value must be a JSON array".to_string());
    }

    let elements = parse_postgres_array_elements(input)?;
    elements
        .into_iter()
        .map(|element| match element {
            ArrayElement::Null => Ok(JsonValue::Null),
            ArrayElement::Value(value) => parse_element(value, element_type),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(JsonValue::Array)
}

enum ArrayElement {
    Null,
    Value(String),
}

fn parse_postgres_array_elements(input: &str) -> Result<Vec<ArrayElement>, String> {
    let mut characters = input.chars().peekable();
    skip_whitespace(&mut characters);
    if characters.next() != Some('{') {
        return Err("PostgreSQL array literal must begin with '{'".to_string());
    }
    skip_whitespace(&mut characters);
    if characters.peek() == Some(&'}') {
        characters.next();
        skip_whitespace(&mut characters);
        return characters
            .next()
            .is_none()
            .then(Vec::new)
            .ok_or_else(|| "trailing data after PostgreSQL array literal".to_string());
    }

    let mut elements = Vec::new();
    loop {
        let (value, quoted) = if characters.peek() == Some(&'"') {
            (parse_quoted_element(&mut characters)?, true)
        } else {
            (parse_unquoted_element(&mut characters)?, false)
        };
        if !quoted && value.eq_ignore_ascii_case("NULL") {
            elements.push(ArrayElement::Null);
        } else {
            elements.push(ArrayElement::Value(value));
        }

        skip_whitespace(&mut characters);
        match characters.next() {
            Some(',') => {
                skip_whitespace(&mut characters);
                if characters.peek().is_none() || characters.peek() == Some(&'}') {
                    return Err("empty PostgreSQL array element".to_string());
                }
            }
            Some('}') => {
                skip_whitespace(&mut characters);
                if characters.next().is_some() {
                    return Err("trailing data after PostgreSQL array literal".to_string());
                }
                break;
            }
            _ => return Err("invalid PostgreSQL array delimiter".to_string()),
        }
    }

    Ok(elements)
}

fn parse_quoted_element(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, String> {
    characters.next();
    let mut value = String::new();
    loop {
        match characters.next() {
            Some('"') => return Ok(value),
            Some('\\') => value.push(
                characters
                    .next()
                    .ok_or_else(|| "unterminated escape in PostgreSQL array".to_string())?,
            ),
            Some(character) => value.push(character),
            None => return Err("unterminated quoted PostgreSQL array element".to_string()),
        }
    }
}

fn parse_unquoted_element(
    characters: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, String> {
    let mut value = String::new();
    let mut trailing_whitespace = String::new();
    loop {
        match characters.peek().copied() {
            Some(',' | '}') => break,
            Some('"' | '{') => {
                return Err("invalid character in unquoted PostgreSQL array element".to_string());
            }
            Some('\\') => {
                characters.next();
                value.push_str(&trailing_whitespace);
                trailing_whitespace.clear();
                value.push(
                    characters
                        .next()
                        .ok_or_else(|| "unterminated escape in PostgreSQL array".to_string())?,
                );
            }
            Some(character) if character.is_whitespace() => {
                characters.next();
                trailing_whitespace.push(character);
            }
            Some(character) => {
                characters.next();
                value.push_str(&trailing_whitespace);
                trailing_whitespace.clear();
                value.push(character);
            }
            None => return Err("unterminated PostgreSQL array literal".to_string()),
        }
    }
    if value.is_empty() {
        Err("empty unquoted PostgreSQL array element".to_string())
    } else {
        Ok(value)
    }
}

fn skip_whitespace(characters: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while characters
        .peek()
        .is_some_and(|character| character.is_whitespace())
    {
        characters.next();
    }
}

fn parse_element(value: String, data_type: &DataType) -> Result<JsonValue, String> {
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => value
            .parse::<i64>()
            .map(JsonValue::from)
            .map_err(|_| format!("invalid integer array element '{value}'")),
        DataType::Float => {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| format!("invalid float array element '{value}'"))?;
            serde_json::Number::from_f64(parsed)
                .map(JsonValue::Number)
                .ok_or_else(|| "non-finite float array element".to_string())
        }
        DataType::Boolean => match value.to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "on" | "1" => Ok(JsonValue::Bool(true)),
            "false" | "f" | "no" | "n" | "off" | "0" => Ok(JsonValue::Bool(false)),
            _ => Err(format!("invalid boolean array element '{value}'")),
        },
        DataType::Json | DataType::Vector(_) => serde_json::from_str(&value)
            .map_err(|error| format!("invalid JSON array element: {error}")),
        DataType::Array(_) => Err("array-of-array types are not supported".to_string()),
        DataType::Null
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp => Ok(JsonValue::String(value)),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_text_array;
    use crate::types::DataType;

    #[test]
    fn should_parse_postgres_text_array_quoted_escapes() {
        // Arrange
        let text = r#"{plain,"comma,value","escaped\"quote"}"#;

        // Act
        let values = parse_text_array(text, &DataType::Text).expect("parse array");

        // Assert
        assert_eq!(
            values,
            serde_json::json!(["plain", "comma,value", "escaped\"quote"])
        );
    }

    #[test]
    fn should_distinguish_postgres_array_null_spellings() {
        // Arrange
        let text = r#"{NULL,"NULL"}"#;

        // Act
        let values = parse_text_array(text, &DataType::Text).expect("parse array");

        // Assert
        assert_eq!(values, serde_json::json!([null, "NULL"]));
    }

    #[test]
    fn should_parse_postgres_integer_array_elements_as_numbers() {
        // Arrange
        let text = "{7,-8}";

        // Act
        let values = parse_text_array(text, &DataType::Int).expect("parse array");

        // Assert
        assert_eq!(values, serde_json::json!([7, -8]));
    }

    #[test]
    fn should_preserve_escaped_trailing_whitespace_in_unquoted_elements() {
        // Arrange
        let text = r"{value\ ,next}";

        // Act
        let values = parse_text_array(text, &DataType::Text).expect("parse array");

        // Assert
        assert_eq!(values, serde_json::json!(["value ", "next"]));
    }

    #[test]
    fn should_keep_json_array_text_input_compatible() {
        // Arrange
        let text = "[7,-8]";

        // Act
        let values = parse_text_array(text, &DataType::Int).expect("parse array");

        // Assert
        assert_eq!(values, serde_json::json!([7, -8]));
    }
}
