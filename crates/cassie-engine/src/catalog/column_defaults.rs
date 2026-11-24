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

use cassie_sql::default_expression::VolatileDefault;
pub use cassie_sql::default_expression::parse_volatile_default_expression;

/// Whether `expression` is a supported volatile default whose value a column
/// of `data_type` can hold.
#[must_use]
pub fn volatile_default_accepts(expression: &str, data_type: &DataType) -> bool {
    let Some(default) = VolatileDefault::parse(expression) else { return false; };
    let textual = matches!(data_type, DataType::Text | DataType::Varchar { .. });
    match default {
        VolatileDefault::Now | VolatileDefault::CurrentTimestamp | VolatileDefault::LocalTimestamp => {
            textual || matches!(data_type, DataType::Timestamp | DataType::Extended(
                crate::types::extended::ExtendedType::Timestamp { .. }
                    | crate::types::extended::ExtendedType::TimestampTz { .. }
            ))
        }
        VolatileDefault::CurrentDate => textual || matches!(data_type, DataType::Date),
        VolatileDefault::RandomUuid => textual || matches!(data_type, DataType::Uuid),
    }
}

/// Evaluates a stored volatile default for one inserted row. Returns `None`
/// when `expression` is not a volatile default (for example a `nextval`
/// expression, which is evaluated through its sequence).
#[must_use]
pub fn evaluate_volatile_default(expression: &str) -> Option<Value> {
    match VolatileDefault::parse(expression)? {
        VolatileDefault::Now | VolatileDefault::CurrentTimestamp | VolatileDefault::LocalTimestamp => {
            let now = time::OffsetDateTime::now_utc();
            Some(Value::String(format!(
                "{}T{:02}:{:02}:{:02}.{:06}Z",
                crate::types::temporal::format_date(now.date()),
                now.hour(), now.minute(), now.second(), now.microsecond()
            )))
        }
        VolatileDefault::CurrentDate => Some(Value::String(crate::types::temporal::format_date(
            time::OffsetDateTime::now_utc().date()
        ))),
        VolatileDefault::RandomUuid => Some(Value::String(uuid::Uuid::new_v4().to_string())),
    }
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
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::ObjectId(_) => {
            coerce_integer(value)
        }
        DataType::Float => coerce_float(value),
        DataType::Extended(kind) => kind
            .from_json(&value)
            .map(|typed| typed.to_json())
            .map_err(|error| error.to_string()),
        DataType::Name => coerce_text(value, "name", |text| {
            Ok(crate::types::name::parse_text(text))
        }),
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
        DataType::Json | DataType::Jsonb => Ok(match value {
            Value::String(text) => {
                crate::types::array::parse_json_document(&text).unwrap_or(Value::String(text))
            }
            other => other,
        }),
        DataType::Composite(_) => {
            Err("DEFAULT values for composite columns are pending under #828".to_string())
        }
        DataType::Domain(descriptor) => coerce_default_literal(&descriptor.base, value),
        DataType::Enum(descriptor) => coerce_enum_default(descriptor, value),
        DataType::Array(_) => coerce_array_default(data_type, value),
        DataType::Null | DataType::Bytea | DataType::JsonPath | DataType::Vector(_) => Ok(value),
    }
}

/// An array column default written as a quoted literal such as `'{1,2}'` is
/// stored in the array storage form that a written row carries.
fn coerce_array_default(data_type: &DataType, value: Value) -> Result<Value, String> {
    let Value::String(text) = value else {
        return Ok(value);
    };
    match crate::executor::filter::cast_value(&crate::types::Value::String(text), data_type) {
        Ok(crate::types::Value::Array(array)) => Ok(array.to_storage_json()),
        Ok(_) => Err("array DEFAULT literal did not produce an array".to_string()),
        Err(error) => Err(error.to_string()),
    }
}

/// An enum column default is stored as the member's storage form, so a later
/// `RENAME VALUE` keeps the default pointing at the same member. A value that
/// already is a storage form of a member of `descriptor` is kept.
fn coerce_enum_default(descriptor: &crate::types::EnumRef, value: Value) -> Result<Value, String> {
    match value {
        Value::String(text) => {
            match crate::executor::filter::enum_cast::cast_text_to_enum(descriptor, &text) {
                Ok(crate::types::Value::Typed(crate::types::typed::TypedValue::Enum(member))) => {
                    Ok(member.to_storage_json())
                }
                Ok(_) => Err(format!(
                    "invalid input value for enum {}: \"{text}\"",
                    descriptor.name
                )),
                Err(error) => Err(error.to_string()),
            }
        }
        other => {
            let known = crate::types::EnumValue::storage_member_id(&other)
                .is_some_and(|member_id| descriptor.member(member_id).is_some());
            if known {
                Ok(other)
            } else {
                Err(format!(
                    "invalid input value for enum {}: \"{other}\"",
                    descriptor.name
                ))
            }
        }
    }
}

fn coerce_boolean(value: Value) -> Result<Value, String> {
    match value {
        Value::Bool(_) => Ok(value),
        Value::String(text) => crate::types::boolean::parse_text(&text)
            .map(Value::Bool)
            .ok_or_else(|| format!("invalid input syntax for type boolean: \"{text}\"")),
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

    fn mood() -> crate::types::EnumRef {
        crate::types::EnumRef::new(crate::types::EnumType {
            oid: 90_001,
            array_oid: 90_002,
            qualified_name: "db.public.mood".to_owned(),
            name: "mood".to_owned(),
            revision: 1,
            members: vec![crate::types::EnumMember {
                member_id: 7,
                label: "sad".to_owned(),
                sort_order: 1.0,
                uncommitted: false,
            }],
        })
    }

    #[test]
    fn should_keep_an_enum_default_that_is_already_a_member_storage_form() {
        // Arrange
        let stored = json!({ crate::types::enum_type::ENUM_STORAGE_KEY: 7 });

        // Act
        let coerced = coerce_default_literal(&DataType::Enum(mood()), stored.clone());

        // Assert
        assert_eq!(coerced, Ok(stored));
    }

    #[test]
    fn should_reject_an_enum_default_that_is_not_a_member() {
        // Arrange
        let unknown = json!({ crate::types::enum_type::ENUM_STORAGE_KEY: 99 });
        let number = json!(5);

        // Act
        let unknown_member = coerce_default_literal(&DataType::Enum(mood()), unknown);
        let wrong_kind = coerce_default_literal(&DataType::Enum(mood()), number);

        // Assert
        assert!(unknown_member.is_err());
        assert!(wrong_kind.is_err());
    }
}
