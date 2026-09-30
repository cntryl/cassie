//! Column `DEFAULT` values.
//!
//! A literal default is coerced to the column's declared type when the DDL is
//! bound, so a table the engine accepts can always be filled by an INSERT that
//! omits the column. A small set of volatile default functions (`now()`,
//! `CURRENT_TIMESTAMP`, `CURRENT_DATE`, `gen_random_uuid()`, ...) is stored
//! as a canonical expression and evaluated once per inserted row. Any other
//! expression is rejected when the DDL is parsed.

use crate::types::DataType;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolatileDefault {
    Now,
    CurrentTimestamp,
    LocalTimestamp,
    CurrentDate,
    RandomUuid,
}

impl VolatileDefault {
    fn parse(raw: &str) -> Option<Self> {
        let normalized = raw
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        // `DEFAULT (now())` names the same function; only exact names match
        // below, so peeling outer parentheses cannot admit an expression.
        let mut unwrapped = normalized.as_str();
        while let Some(inner) = unwrapped
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            unwrapped = inner;
        }
        let without_precision = strip_precision(unwrapped);
        match without_precision {
            "now()" => Some(Self::Now),
            "current_timestamp" => Some(Self::CurrentTimestamp),
            "localtimestamp" => Some(Self::LocalTimestamp),
            "current_date" => Some(Self::CurrentDate),
            "gen_random_uuid()" => Some(Self::RandomUuid),
            _ => None,
        }
    }

    const fn canonical(self) -> &'static str {
        match self {
            Self::Now => "now()",
            Self::CurrentTimestamp => "CURRENT_TIMESTAMP",
            Self::LocalTimestamp => "LOCALTIMESTAMP",
            Self::CurrentDate => "CURRENT_DATE",
            Self::RandomUuid => "gen_random_uuid()",
        }
    }

    fn accepts(self, data_type: &DataType) -> bool {
        let textual = matches!(data_type, DataType::Text | DataType::Varchar { .. });
        match self {
            Self::Now | Self::CurrentTimestamp | Self::LocalTimestamp => {
                textual || matches!(data_type, DataType::Timestamp)
            }
            Self::CurrentDate => textual || matches!(data_type, DataType::Date),
            Self::RandomUuid => textual || matches!(data_type, DataType::Uuid),
        }
    }

    fn evaluate(self) -> Value {
        match self {
            Self::Now | Self::CurrentTimestamp | Self::LocalTimestamp => {
                let now = time::OffsetDateTime::now_utc();
                Value::String(format!(
                    "{}T{:02}:{:02}:{:02}.{:06}Z",
                    crate::types::temporal::format_date(now.date()),
                    now.hour(),
                    now.minute(),
                    now.second(),
                    now.microsecond()
                ))
            }
            Self::CurrentDate => Value::String(crate::types::temporal::format_date(
                time::OffsetDateTime::now_utc().date(),
            )),
            Self::RandomUuid => Value::String(uuid::Uuid::new_v4().to_string()),
        }
    }
}

/// Drops a `(precision)` suffix from `current_timestamp(3)` or
/// `localtimestamp(3)`; any other text is returned unchanged.
fn strip_precision(normalized: &str) -> &str {
    let Some(open) = normalized.find('(') else {
        return normalized;
    };
    let (name, rest) = normalized.split_at(open);
    let digits = rest
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_default();
    if matches!(name, "current_timestamp" | "localtimestamp")
        && !digits.is_empty()
        && digits.chars().all(|character| character.is_ascii_digit())
    {
        name
    } else {
        normalized
    }
}

/// Returns the canonical stored expression for a supported volatile column
/// default such as `now()` or `CURRENT_TIMESTAMP`, or `None` for any other
/// text.
#[must_use]
pub fn parse_volatile_default_expression(raw: &str) -> Option<String> {
    VolatileDefault::parse(raw).map(|default| default.canonical().to_string())
}

/// Whether `expression` is a supported volatile default whose value a column
/// of `data_type` can hold.
#[must_use]
pub fn volatile_default_accepts(expression: &str, data_type: &DataType) -> bool {
    VolatileDefault::parse(expression).is_some_and(|default| default.accepts(data_type))
}

/// Evaluates a stored volatile default for one inserted row. Returns `None`
/// when `expression` is not a volatile default (for example a `nextval`
/// expression, which is evaluated through its sequence).
#[must_use]
pub fn evaluate_volatile_default(expression: &str) -> Option<Value> {
    VolatileDefault::parse(expression).map(VolatileDefault::evaluate)
}

/// Coerces a literal column default to the column's declared type, as
/// PostgreSQL coerces an untyped literal when it binds `DEFAULT`.
///
/// # Errors
///
/// Returns a message when the literal cannot be represented as `data_type`.
pub fn coerce_default_literal(data_type: &DataType, value: Value) -> Result<Value, String> {
    if value.is_null() {
        return Ok(value);
    }
    match data_type {
        DataType::Boolean => coerce_boolean(value),
        DataType::SmallInt | DataType::Int | DataType::BigInt => coerce_integer(value),
        DataType::Float => coerce_float(value),
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => Ok(match value {
            Value::Number(number) => Value::String(number.to_string()),
            Value::Bool(flag) => Value::String(flag.to_string()),
            other => other,
        }),
        DataType::Date => coerce_text(value, "date", crate::types::temporal::canonical_date),
        DataType::Time => coerce_text(value, "time", crate::types::temporal::canonical_time),
        DataType::Timestamp => coerce_text(
            value,
            "timestamp",
            crate::types::temporal::canonical_timestamp,
        ),
        DataType::Uuid => coerce_text(value, "uuid", |text| {
            uuid::Uuid::parse_str(text.trim())
                .map(|parsed| parsed.to_string())
                .map_err(|error| error.to_string())
        }),
        DataType::Json => Ok(match value {
            Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
            other => other,
        }),
        DataType::Null | DataType::Bytea | DataType::Vector(_) | DataType::Array(_) => Ok(value),
    }
}

fn coerce_boolean(value: Value) -> Result<Value, String> {
    match value {
        Value::Bool(_) => Ok(value),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "t" | "true" | "y" | "yes" | "on" | "1" => Ok(Value::Bool(true)),
            "f" | "false" | "n" | "no" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(format!("invalid input syntax for type boolean: \"{text}\"")),
        },
        other => Err(format!(
            "default for a boolean column must be a boolean, got {other}"
        )),
    }
}

fn coerce_integer(value: Value) -> Result<Value, String> {
    match value {
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(Value::Number(number)),
        // An assignment cast from numeric to integer rounds half away from
        // zero, so `INT DEFAULT 2.5` stores 3 as in PostgreSQL.
        Value::Number(number) => number
            .as_f64()
            .map(f64::round)
            .filter(|rounded| rounded.is_finite())
            .and_then(|rounded| format!("{rounded:.0}").parse::<i64>().ok())
            .map(Value::from)
            .ok_or_else(|| format!("integer out of range: {number}")),
        Value::String(text) => text
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| format!("invalid input syntax for type integer: \"{text}\"")),
        other => Err(format!(
            "default for an integer column must be an integer, got {other}"
        )),
    }
}

fn coerce_float(value: Value) -> Result<Value, String> {
    match value {
        Value::Number(_) => Ok(value),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("invalid input syntax for type double precision: \"{text}\"")),
        other => Err(format!(
            "default for a float column must be numeric, got {other}"
        )),
    }
}

fn coerce_text(
    value: Value,
    type_name: &str,
    canonical: impl Fn(&str) -> Result<String, String>,
) -> Result<Value, String> {
    let Value::String(text) = value else {
        return Err(format!(
            "default for a {type_name} column must be a {type_name} literal, got {value}"
        ));
    };
    canonical(&text)
        .map(Value::String)
        .map_err(|_| format!("invalid input syntax for type {type_name}: \"{text}\""))
}

#[cfg(test)]
mod tests {
    use super::{
        coerce_default_literal, evaluate_volatile_default, parse_volatile_default_expression,
        volatile_default_accepts,
    };
    use crate::types::DataType;
    use serde_json::{json, Value};

    #[test]
    fn should_coerce_quoted_boolean_default_spellings() {
        // Arrange
        let spellings = ["true", "t", "f", "yes", "off", "1"];

        // Act
        let coerced = spellings
            .iter()
            .map(|spelling| coerce_default_literal(&DataType::Boolean, json!(spelling)))
            .collect::<Result<Vec<_>, _>>()
            .expect("boolean spellings coerce");

        // Assert
        assert_eq!(
            coerced,
            vec![
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(true),
            ]
        );
    }

    #[test]
    fn should_reject_numeric_default_for_boolean_column() {
        // Arrange
        let value = json!(1);

        // Act
        let coerced = coerce_default_literal(&DataType::Boolean, value);

        // Assert
        assert!(coerced.is_err());
    }

    #[test]
    fn should_canonicalize_volatile_default_spellings() {
        // Arrange
        let spellings = [
            "NOW()",
            "current_timestamp(3)",
            "CURRENT_DATE",
            "gen_random_uuid ()",
        ];

        // Act
        let canonical = spellings
            .iter()
            .map(|spelling| parse_volatile_default_expression(spelling))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            canonical,
            vec![
                Some("now()".to_string()),
                Some("CURRENT_TIMESTAMP".to_string()),
                Some("CURRENT_DATE".to_string()),
                Some("gen_random_uuid()".to_string()),
            ]
        );
        assert_eq!(parse_volatile_default_expression("abs(-3)"), None);
    }

    #[test]
    fn should_evaluate_random_uuid_default_to_a_fresh_uuid() {
        // Arrange
        let expression = "gen_random_uuid()";

        // Act
        let first = evaluate_volatile_default(expression).expect("uuid default");
        let second = evaluate_volatile_default(expression).expect("uuid default");

        // Assert
        assert_ne!(first, second);
        assert!(volatile_default_accepts(expression, &DataType::Uuid));
        assert!(!volatile_default_accepts(expression, &DataType::Int));
    }

    #[test]
    fn should_round_numeric_default_for_integer_column() {
        // Arrange
        let values = [json!(2.5), json!(-2.5), json!(3.0)];

        // Act
        let coerced = values
            .into_iter()
            .map(|value| coerce_default_literal(&DataType::Int, value))
            .collect::<Result<Vec<_>, _>>()
            .expect("numeric defaults coerce");

        // Assert
        assert_eq!(coerced, vec![json!(3), json!(-3), json!(3)]);
    }

    #[test]
    fn should_recognize_parenthesized_volatile_default() {
        // Arrange
        let spellings = ["(now())", "((CURRENT_DATE))", "(now()) + (now())"];

        // Act
        let canonical = spellings
            .iter()
            .map(|spelling| parse_volatile_default_expression(spelling))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            canonical,
            vec![
                Some("now()".to_string()),
                Some("CURRENT_DATE".to_string()),
                None
            ]
        );
    }
}
