//! Borrowed unknown-literal conversion checks at ordered binding boundaries.
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CastIdentity, Expr};
use crate::sql::parameters::ScalarLiteralDiagnostic;
use crate::types::{DataType, IntegerWidth};

pub(crate) fn validate(
    expression: &Expr,
    target: &DataType,
    identity: CastIdentity,
) -> Result<(), CassieError> {
    validate_with_controls(expression, target, identity, None)
}

pub(crate) fn validate_with_controls(
    expression: &Expr,
    target: &DataType,
    identity: CastIdentity,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), CassieError> {
    let Expr::StringLiteral(input) = expression else {
        return Ok(());
    };
    if !has_input_law(target, identity) {
        return Ok(());
    }
    if numeric_input_law(target, identity) {
        use crate::types::extended::numeric_validation::{validate, ValidationError};
        return match validate(input, controls) {
            Ok(()) => Ok(()),
            Err(ValidationError::Interrupted(error)) => Err(error),
            Err(ValidationError::Syntax) => match controls {
                Some(controls) => Err(CassieError::OwnedNumericLiteral(
                    crate::app::OwnedNumericLiteralError::syntax(input, controls)?,
                )),
                None => Err(CassieError::TypeInput(
                    crate::types::extended::TypeInputError::new(
                        "22P02",
                        format!("invalid input syntax for type numeric: \"{input}\""),
                    ),
                )),
            },
            Err(ValidationError::Range) => match controls {
                Some(controls) => Err(CassieError::OwnedNumericLiteral(
                    crate::app::OwnedNumericLiteralError::range(controls)?,
                )),
                None => Err(CassieError::TypeInput(
                    crate::types::extended::TypeInputError::new(
                        "22003",
                        "value overflows numeric format".into(),
                    ),
                )),
            },
        };
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    if *target == DataType::Bytea {
        return crate::types::bytea_input::validate(input).map_err(CassieError::from);
    }
    if *target == DataType::JsonPath {
        return sql_json_path::JsonPath::new(input)
            .map(|_| ())
            .map_err(|error| {
                CassieError::from(crate::app::DiagnosticError::new("42601", error.to_string()))
            });
    }
    let Some(diagnostic) = diagnostic(input, target) else {
        return Ok(());
    };
    let bytes = diagnostic.capacity(input).ok_or_else(|| {
        CassieError::ResourceLimit("scalar input diagnostic capacity overflow".into())
    })?;
    let mut reservation = controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()?;
    let mut message = String::new();
    message.try_reserve_exact(bytes).map_err(|error| {
        CassieError::ResourceLimit(format!(
            "unable to allocate scalar input diagnostic: {error}"
        ))
    })?;
    if message.capacity() > bytes {
        if let Some(reservation) = &mut reservation {
            reservation.try_grow(message.capacity() - bytes)?;
        }
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    diagnostic
        .write(input, &mut message)
        .expect("String formatting cannot fail");
    Err(CassieError::Execution(message))
}

pub(crate) fn has_input_law(target: &DataType, identity: CastIdentity) -> bool {
    // Preserve the existing quoted NUMERIC/DECIMAL Float adapter. This does not
    // qualify an exact NUMERIC carrier or raw numeric literals.
    identity == CastIdentity::Scalar
        || (identity == CastIdentity::Numeric && *target == DataType::Float)
        || numeric_input_law(target, identity)
}

fn numeric_input_law(target: &DataType, identity: CastIdentity) -> bool {
    identity == CastIdentity::Numeric
        && matches!(
            target,
            DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
        )
}

pub(crate) fn rejects_cast_with_controls(
    expression: &Expr,
    controls: Option<&QueryExecutionControls>,
) -> Result<bool, CassieError> {
    let Expr::Cast {
        expr,
        data_type,
        identity,
        ..
    } = expression
    else {
        return Ok(false);
    };
    let Expr::StringLiteral(input) = expr.as_ref() else {
        return Ok(false);
    };
    if !has_input_law(data_type, *identity) {
        return Ok(false);
    }
    if numeric_input_law(data_type, *identity) {
        use crate::types::extended::numeric_validation::{validate, ValidationError};
        return match validate(input, controls) {
            Ok(()) => Ok(false),
            Err(ValidationError::Syntax | ValidationError::Range) => Ok(true),
            Err(ValidationError::Interrupted(error)) => Err(error),
        };
    }
    Ok(if *data_type == DataType::Bytea {
        crate::types::bytea_input::validate(input).is_err()
    } else {
        diagnostic(input, data_type).is_some()
    })
}

pub(crate) fn diagnostic(input: &str, target: &DataType) -> Option<ScalarLiteralDiagnostic> {
    use ScalarLiteralDiagnostic::{Range, Syntax};
    if let Some(width) = IntegerWidth::from_data_type(target) {
        let name = match width {
            IntegerWidth::I16 => "smallint",
            IntegerWidth::I32 => "integer",
            IntegerWidth::I64 => "bigint",
        };
        return crate::types::integer::parse(input, width)
            .err()
            .map(|error| match error {
                crate::types::integer::IntegerInputError::Syntax => Syntax(name),
                crate::types::integer::IntegerInputError::Range => Range(name),
            });
    }
    match target {
        DataType::Boolean => crate::types::boolean::parse_text(input)
            .is_none()
            .then_some(Syntax("boolean")),
        DataType::Uuid => crate::types::uuid_text::parse(input)
            .is_none()
            .then_some(Syntax("uuid")),
        DataType::Float => crate::types::float_input::parse(input)
            .err()
            .map(|error| match error {
                crate::types::FloatInputError::Syntax => Syntax("double precision"),
                crate::types::FloatInputError::Range => ScalarLiteralDiagnostic::FloatRange,
            }),
        DataType::Json | DataType::Jsonb => serde_json::from_str::<serde::de::IgnoredAny>(input)
            .err()
            .map(|_| Syntax("json")),
        _ => None,
    }
}

#[cfg(test)]
#[path = "literal_inputs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "literal_inputs_numeric_tests.rs"]
mod numeric_tests;
