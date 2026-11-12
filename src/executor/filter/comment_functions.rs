//! Selected catalog description lookup functions.

use super::{CassieSession, QueryError, Value};

pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    match name {
        "obj_description" | "pg_catalog.obj_description" => {
            Some(evaluate_object_description(name, args, session))
        }
        "col_description" | "pg_catalog.col_description" => {
            Some(evaluate_column_description(name, args, session))
        }
        _ => None,
    }
}
fn evaluate_object_description(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    super::functions::require_arg_count_range(name, args, 1..=2)?;
    let Some(object_oid) = oid_argument(name, &args[0])? else {
        return Ok(Value::Null);
    };
    let Some((catalog, context)) = session.and_then(CassieSession::object_lookup_context) else {
        return Ok(Value::Null);
    };
    let description = if let Some(class_argument) = args.get(1) {
        let Some(class_name) = super::functions::text_arg(name, class_argument)? else {
            return Ok(Value::Null);
        };
        let class_oid = match class_name.as_str() {
            "pg_class" => 1259,
            "pg_namespace" => 2615,
            _ => return Ok(Value::Null),
        };
        catalog.object_comment(&context.database, class_oid, object_oid, 0)
    } else {
        catalog.object_comment_by_oid(&context.database, object_oid)
    };
    Ok(description.map_or(Value::Null, Value::String))
}

fn evaluate_column_description(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Result<Value, QueryError> {
    super::functions::require_arg_count(name, args, 2)?;
    let Some(object_oid) = oid_argument(name, &args[0])? else {
        return Ok(Value::Null);
    };
    let Some(object_sub_id) = attribute_number(name, &args[1])? else {
        return Ok(Value::Null);
    };
    if object_sub_id <= 0 {
        return Ok(Value::Null);
    }
    let Some((catalog, context)) = session.and_then(CassieSession::object_lookup_context) else {
        return Ok(Value::Null);
    };
    let description = catalog.object_comment(&context.database, 1259, object_oid, object_sub_id);
    Ok(description.map_or(Value::Null, Value::String))
}

fn oid_argument(name: &str, value: &Value) -> Result<Option<u32>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) => crate::types::object_id::oid_from_integer(*value)
            .map(Some)
            .map_err(|value| QueryError::General(format!("OID out of range: {value}"))),
        _ => Err(QueryError::General(format!(
            "function '{name}' expects an oid input"
        ))),
    }
}

fn integer_argument(name: &str, value: &Value) -> Result<Option<i64>, QueryError> {
    match value {
        Value::Null => Ok(None),
        Value::Int64(value) => Ok(Some(*value)),
        _ => Err(QueryError::General(format!(
            "function '{name}' expects an integer input"
        ))),
    }
}

fn attribute_number(name: &str, value: &Value) -> Result<Option<i32>, QueryError> {
    integer_argument(name, value)?
        .map(|value| {
            i32::try_from(value).map_err(|_| {
                QueryError::Cassie(crate::app::CassieError::Execution(
                    "integer out of range".to_owned(),
                ))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::attribute_number;
    use crate::types::Value;

    #[test]
    fn should_reject_out_of_range_attribute_number_given_int4_catalog_signature() {
        // Arrange
        let value = Value::Int64(i64::from(i32::MAX) + 1);

        // Act
        let result = attribute_number("col_description", &value);

        // Assert
        assert!(
            matches!(result, Err(crate::executor::QueryError::Cassie(crate::app::CassieError::Execution(message))) if message == "integer out of range")
        );
    }

    #[test]
    fn should_keep_null_attribute_number_null_given_nullable_int4_input() {
        // Arrange
        let value = Value::Null;

        // Act
        let result = attribute_number("col_description", &value);

        // Assert
        assert!(matches!(result, Ok(None)));
    }
}
