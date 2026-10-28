use serde_json::Value as JsonValue;

use crate::types::DataType;

mod elements;

/// Distinguishes malformed input from a selected unsupported element origin.
#[derive(Debug)]
pub(crate) enum TextArrayError {
    Invalid(String),
    UnsupportedJsonDocumentNull,
}

pub(crate) struct ParsedTextArray {
    pub(crate) value: JsonValue,
    pub(crate) flat_values: JsonValue,
    pub(crate) flat_elements: Vec<ParsedTextArrayElement>,
    pub(crate) dimensions: Vec<crate::types::ArrayDimension>,
}

#[derive(Debug, Clone)]
pub(crate) enum ParsedTextArrayElement {
    SqlNull,
    Value(JsonValue),
}

impl std::fmt::Display for TextArrayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::UnsupportedJsonDocumentNull => formatter
                .write_str("JSON document-null scalar elements in SQL arrays are not supported"),
        }
    }
}

pub(crate) fn parse_text_array(
    input: &str,
    element_type: &DataType,
) -> Result<JsonValue, TextArrayError> {
    let parsed = parse_text_array_with_dimensions(input, element_type)?;
    if !input.trim_start().starts_with('[')
        && matches!(element_type, DataType::Json | DataType::Jsonb)
        && parsed.flat_elements.iter().any(
            |element| matches!(element, ParsedTextArrayElement::Value(value) if value.is_null()),
        )
    {
        return Err(TextArrayError::UnsupportedJsonDocumentNull);
    }
    Ok(parsed.value)
}

pub(crate) fn parse_sql_text_array(
    input: &str,
    element_type: &DataType,
) -> Result<ParsedTextArray, TextArrayError> {
    let input = input.trim();
    let (dimensions, _) = parse_dimension_prefix(input)?;
    if !input.starts_with('{') && dimensions.is_empty() {
        return Err(TextArrayError::Invalid(
            "malformed array literal: array value must start with '{' or dimensions".to_string(),
        ));
    }
    parse_text_array_with_dimensions(input, element_type)
}

pub(crate) fn parse_text_array_with_dimensions(
    input: &str,
    element_type: &DataType,
) -> Result<ParsedTextArray, TextArrayError> {
    let mut element_type = element_type;
    while let DataType::Array(inner) = element_type {
        element_type = inner;
    }
    let input = input.trim();
    let (explicit_dimensions, body) = parse_dimension_prefix(input)?;
    if explicit_dimensions.is_empty() && input.starts_with('[') {
        return parse_json_text_array(input, element_type);
    }

    let parsed = PostgresArrayParser::new(body)
        .parse()
        .map_err(TextArrayError::Invalid)?;
    let mut values = Vec::with_capacity(parsed.elements.len());
    let parsed_dimensions = parsed.dimensions;
    let mut flat_elements = Vec::with_capacity(parsed.elements.len());
    for element in parsed.elements {
        let value = match element {
            ArrayElement::Null => {
                flat_elements.push(ParsedTextArrayElement::SqlNull);
                JsonValue::Null
            }
            ArrayElement::Value(value) => {
                let parsed = parse_element(value, element_type).map_err(TextArrayError::Invalid)?;
                flat_elements.push(ParsedTextArrayElement::Value(parsed.clone()));
                parsed
            }
        };
        values.push(value);
    }
    let flat_values = JsonValue::Array(values.clone());
    let value = nest_array_values(values, &parsed_dimensions);
    let dimensions = if explicit_dimensions.is_empty() {
        parsed_dimensions
    } else {
        if !parsed_dimensions.is_empty()
            && (parsed_dimensions.len() != explicit_dimensions.len()
                || parsed_dimensions
                    .iter()
                    .zip(&explicit_dimensions)
                    .any(|(actual, declared)| actual.length != declared.length))
        {
            return Err(TextArrayError::Invalid(
                "array dimensions do not match array contents".to_string(),
            ));
        }
        explicit_dimensions
    };
    Ok(ParsedTextArray {
        value,
        flat_values,
        flat_elements,
        dimensions,
    })
}

fn parse_json_text_array(
    input: &str,
    element_type: &DataType,
) -> Result<ParsedTextArray, TextArrayError> {
    if matches!(
        element_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float
    ) {
        let value = elements::parse_json_numeric_array(input, element_type)
            .map_err(TextArrayError::Invalid)?;
        return parsed_json_array(value);
    }
    let value: JsonValue = serde_json::from_str(input)
        .map_err(|error| TextArrayError::Invalid(format!("invalid JSON array: {error}")))?;
    let JsonValue::Array(values) = value else {
        return Err(TextArrayError::Invalid(
            "array value must be a JSON array".to_string(),
        ));
    };
    let value = values
        .into_iter()
        .map(|value| elements::normalize_element(value, element_type))
        .collect::<Result<Vec<_>, _>>()
        .map(JsonValue::Array)
        .map_err(TextArrayError::Invalid)?;
    parsed_json_array(value)
}

fn parsed_json_array(value: JsonValue) -> Result<ParsedTextArray, TextArrayError> {
    let length = value.as_array().map_or(0, Vec::len);
    let dimensions = if length == 0 {
        Vec::new()
    } else {
        vec![dimension(length, 1).map_err(TextArrayError::Invalid)?]
    };
    let flat_elements = value
        .as_array()
        .expect("JSON array input was normalized")
        .iter()
        .cloned()
        .map(ParsedTextArrayElement::Value)
        .collect();
    Ok(ParsedTextArray {
        flat_values: value.clone(),
        flat_elements,
        value,
        dimensions,
    })
}

impl ParsedTextArray {
    pub(crate) fn into_sql_array(
        self,
        element_type: &DataType,
    ) -> Result<crate::types::SqlArray, String> {
        let mut element_type = element_type;
        while let DataType::Array(inner) = element_type {
            element_type = inner;
        }
        let json_elements = matches!(element_type, DataType::Json | DataType::Jsonb);
        let source = if json_elements {
            &self.flat_values
        } else {
            &self.value
        };
        let base = if json_elements {
            crate::types::SqlArray::from_nested_json(source, true)?
        } else {
            crate::types::SqlArray::from_nested_json_typed(source, element_type)?
        };
        let mut elements = base.elements().to_vec();
        for (element, parsed) in elements.iter_mut().zip(self.flat_elements) {
            match parsed {
                ParsedTextArrayElement::SqlNull => *element = crate::types::Value::Null,
                ParsedTextArrayElement::Value(value) if json_elements => {
                    *element = crate::types::Value::Json(value);
                }
                ParsedTextArrayElement::Value(_) => {}
            }
        }
        crate::types::SqlArray::new(self.dimensions, elements)
    }
}

fn dimension(length: usize, lower_bound: i32) -> Result<crate::types::ArrayDimension, String> {
    let length = i32::try_from(length).map_err(|_| "array dimension exceeds INT4".to_string())?;
    Ok(crate::types::ArrayDimension {
        length,
        lower_bound,
    })
}

fn nest_array_values(
    values: Vec<JsonValue>,
    dimensions: &[crate::types::ArrayDimension],
) -> JsonValue {
    let lengths = dimensions
        .iter()
        .map(|dimension| {
            usize::try_from(dimension.length).expect("array dimensions are nonnegative")
        })
        .collect::<Vec<_>>();
    nest_array_level(values, &lengths)
}

fn nest_array_level(values: Vec<JsonValue>, lengths: &[usize]) -> JsonValue {
    if lengths.len() <= 1 {
        return JsonValue::Array(values);
    }
    let Some((outer_length, inner_lengths)) = lengths.split_first() else {
        return JsonValue::Array(values);
    };
    let mut values = values.into_iter();
    let mut nested = Vec::with_capacity(*outer_length);
    let child_size = inner_lengths.iter().product::<usize>();
    for _ in 0..*outer_length {
        let child = values.by_ref().take(child_size).collect::<Vec<_>>();
        nested.push(nest_array_level(child, inner_lengths));
    }
    JsonValue::Array(nested)
}

/// Formats the one-dimensional JSON carrier using PostgreSQL array text
/// syntax for a known SQL element type.
pub(crate) fn format_text_array(value: &JsonValue, element_type: &DataType) -> Option<String> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    let elements = values
        .iter()
        .map(|value| {
            if value.is_null() {
                return Some("NULL".to_owned());
            }
            let text = format_element(value, element_type)?;
            Some(quote_element(&text))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{{{}}}", elements.join(",")))
}

pub(crate) fn format_json_array_text(value: &JsonValue) -> Option<String> {
    let JsonValue::Array(values) = value else {
        return None;
    };
    let element_type = if values.is_empty() {
        DataType::Text
    } else if values
        .iter()
        .all(|value| matches!(value, JsonValue::Number(_)))
    {
        if values
            .iter()
            .any(|value| matches!(value, JsonValue::Number(number) if number.is_f64()))
        {
            DataType::Float
        } else {
            DataType::BigInt
        }
    } else if values.iter().all(JsonValue::is_boolean) {
        DataType::Boolean
    } else if values.iter().all(JsonValue::is_string) {
        DataType::Text
    } else {
        DataType::Json
    };
    format_text_array(value, &element_type)
}

fn format_element(value: &JsonValue, element_type: &DataType) -> Option<String> {
    Some(match (value, element_type) {
        (JsonValue::String(value), DataType::Extended(kind)) => kind
            .parse_text(value)
            .map_or_else(|_| value.clone(), |typed| typed.text()),
        (JsonValue::String(value), _) => value.clone(),
        (JsonValue::Number(value), DataType::Float) => {
            crate::types::float_text::format(value.as_f64()?)
        }
        (JsonValue::Number(value), _) => value.to_string(),
        (JsonValue::Bool(value), DataType::Boolean) => if *value { "t" } else { "f" }.to_owned(),
        (JsonValue::Bool(value), _) => value.to_string(),
        (
            JsonValue::Array(_) | JsonValue::Object(_),
            DataType::Json | DataType::Jsonb | DataType::Vector(_),
        ) => value.to_string(),
        (JsonValue::Array(_) | JsonValue::Object(_), _) => return None,
        (JsonValue::Null, _) => "NULL".to_owned(),
    })
}

fn quote_element(value: &str) -> String {
    if value.is_empty()
        || value.eq_ignore_ascii_case("NULL")
        || value.chars().any(|character| {
            character.is_whitespace() || matches!(character, '{' | '}' | ',' | '"' | '\\')
        })
    {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for character in value.chars() {
            if matches!(character, '"' | '\\') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    } else {
        value.to_owned()
    }
}

enum ArrayElement {
    Null,
    Value(String),
}

struct ParsedPostgresArray {
    dimensions: Vec<crate::types::ArrayDimension>,
    elements: Vec<ArrayElement>,
}

struct PostgresArrayParser<'a> {
    characters: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> PostgresArrayParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            characters: input.chars().peekable(),
        }
    }

    fn parse(mut self) -> Result<ParsedPostgresArray, String> {
        skip_whitespace(&mut self.characters);
        let parsed = self.parse_level()?;
        skip_whitespace(&mut self.characters);
        if self.characters.next().is_some() {
            return Err("trailing data after PostgreSQL array literal".to_string());
        }
        Ok(parsed)
    }

    fn parse_level(&mut self) -> Result<ParsedPostgresArray, String> {
        if self.characters.next() != Some('{') {
            return Err("PostgreSQL array literal must begin with '{'".to_string());
        }
        skip_whitespace(&mut self.characters);
        if self.characters.peek() == Some(&'}') {
            self.characters.next();
            return Ok(ParsedPostgresArray {
                dimensions: Vec::new(),
                elements: Vec::new(),
            });
        }

        let mut children = Vec::new();
        let nested = self.characters.peek() == Some(&'{');
        loop {
            if (self.characters.peek() == Some(&'{')) != nested {
                return Err("multidimensional array levels must be rectangular".to_string());
            }
            if nested {
                children.push(self.parse_level()?);
            } else {
                let (value, quoted) = if self.characters.peek() == Some(&'"') {
                    (parse_quoted_element(&mut self.characters)?, true)
                } else {
                    (parse_unquoted_element(&mut self.characters)?, false)
                };
                children.push(ParsedPostgresArray {
                    dimensions: Vec::new(),
                    elements: vec![if !quoted && value.eq_ignore_ascii_case("NULL") {
                        ArrayElement::Null
                    } else {
                        ArrayElement::Value(value)
                    }],
                });
            }
            skip_whitespace(&mut self.characters);
            match self.characters.next() {
                Some(',') => {
                    skip_whitespace(&mut self.characters);
                    if self.characters.peek().is_none() || self.characters.peek() == Some(&'}') {
                        return Err("empty PostgreSQL array element".to_string());
                    }
                }
                Some('}') => break,
                _ => return Err("invalid PostgreSQL array delimiter".to_string()),
            }
        }

        if nested {
            let Some(first_child) = children.first() else {
                return Err("multidimensional array levels must be rectangular".to_string());
            };
            let child_dimensions = first_child.dimensions.clone();
            if child_dimensions.is_empty()
                || children
                    .iter()
                    .any(|child| child.dimensions != child_dimensions)
            {
                return Err("multidimensional array levels must be rectangular".to_string());
            }
            let mut dimensions = vec![dimension(children.len(), 1)?];
            dimensions.extend(child_dimensions);
            Ok(ParsedPostgresArray {
                dimensions,
                elements: children
                    .into_iter()
                    .flat_map(|child| child.elements)
                    .collect(),
            })
        } else {
            let elements: Vec<ArrayElement> = children
                .into_iter()
                .flat_map(|child| child.elements)
                .collect();
            Ok(ParsedPostgresArray {
                dimensions: vec![dimension(elements.len(), 1)?],
                elements,
            })
        }
    }
}

fn parse_dimension_prefix(
    input: &str,
) -> Result<(Vec<crate::types::ArrayDimension>, &str), TextArrayError> {
    let mut cursor = 0;
    let mut dimensions = Vec::new();
    skip_ascii_whitespace(input.as_bytes(), &mut cursor);
    while input.as_bytes().get(cursor) == Some(&b'[') {
        let Some(relative_close) = input[cursor + 1..].find(']') else {
            break;
        };
        let close = cursor + 1 + relative_close;
        let descriptor = input[cursor + 1..close].trim();
        let Some((lower, upper)) = descriptor.split_once(':') else {
            break;
        };
        if !is_integer_bound(lower) || !is_integer_bound(upper) {
            break;
        }
        let lower_bound = lower.trim().parse::<i32>().map_err(|_| {
            TextArrayError::Invalid("invalid PostgreSQL array lower bound".to_string())
        })?;
        let upper_bound = upper.trim().parse::<i32>().map_err(|_| {
            TextArrayError::Invalid("invalid PostgreSQL array upper bound".to_string())
        })?;
        let length = i64::from(upper_bound) - i64::from(lower_bound) + 1;
        if length < 0 {
            return Err(TextArrayError::Invalid(
                "invalid PostgreSQL array dimension bounds".to_string(),
            ));
        }
        dimensions.push(crate::types::ArrayDimension {
            length: i32::try_from(length)
                .map_err(|_| TextArrayError::Invalid("array dimension exceeds INT4".to_string()))?,
            lower_bound,
        });
        cursor = close + 1;
        skip_ascii_whitespace(input.as_bytes(), &mut cursor);
    }
    if dimensions.is_empty() {
        return Ok((dimensions, input));
    }
    if input.as_bytes().get(cursor) != Some(&b'=') {
        return Err(TextArrayError::Invalid(
            "invalid PostgreSQL array dimension prefix".to_string(),
        ));
    }
    Ok((dimensions, &input[cursor + 1..]))
}

fn is_integer_bound(value: &str) -> bool {
    let value = value.trim();
    let digits = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn skip_ascii_whitespace(input: &[u8], cursor: &mut usize) {
    while input.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
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
    let parsed = match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => value
            .trim()
            .parse::<i64>()
            .map(JsonValue::from)
            .map_err(|_| format!("invalid integer array element '{value}'")),
        DataType::ObjectId(_) => crate::types::object_id::parse_oid_text(&value)
            .map(JsonValue::from)
            .map_err(|error| error.to_string()),
        DataType::Extended(kind) => kind
            .parse_text(&value)
            .map(|typed| typed.to_json())
            .map_err(|error| error.to_string()),
        DataType::Float => {
            let parsed = crate::types::float_input::parse(&value)
                .map_err(|error| format!("{error}: '{value}'"))?;
            serde_json::Number::from_f64(parsed)
                .map(JsonValue::Number)
                .ok_or_else(|| "non-finite float array element".to_string())
        }
        DataType::Boolean => crate::types::boolean::parse_text(&value)
            .map(JsonValue::Bool)
            .ok_or_else(|| format!("invalid boolean array element '{value}'")),
        DataType::Json | DataType::Jsonb | DataType::Vector(_) => serde_json::from_str(&value)
            .map_err(|error| format!("invalid JSON array element: {error}")),
        DataType::Array(_) => Err("array-of-array types are not supported".to_string()),
        DataType::JsonPath => Err("JSONPATH arrays are not supported".to_string()),
        DataType::Null
        | DataType::Name
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp => Ok(JsonValue::String(value)),
    }?;
    elements::normalize_element(parsed, data_type)
}

#[cfg(test)]
mod input_contract_tests;

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
