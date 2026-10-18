//! Checked conversions and resolved operators for existing integer domains.
use super::{QueryError, ScalarValue};
use crate::sql::ScalarLiteralDiagnostic;
use crate::types::integer::{self, IntegerArithmeticError, IntegerInputError};
use crate::types::{IntegerOperation, IntegerWidth};

pub(super) fn cast(
    value: &ScalarValue,
    width: IntegerWidth,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let value = match value {
        ScalarValue::Int(value) => *value,
        ScalarValue::Bool(value) => i64::from(*value),
        ScalarValue::Float(value) if value.is_finite() => format!("{:.0}", value.round_ties_even())
            .parse::<i64>()
            .map_err(|_| range(width))?,
        ScalarValue::Str(text) => return controlled_input(text, width, controls),
        ScalarValue::Typed(
            crate::types::typed::TypedValue::Bit(bits)
            | crate::types::typed::TypedValue::VarBit(bits),
        ) => {
            let bits_width = if width == crate::types::IntegerWidth::I64 {
                64
            } else {
                32
            };
            return bits
                .to_integer(bits_width)
                .map(ScalarValue::Int)
                .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error)));
        }
        // `numeric` rounds half away from zero (`numeric_int8`).
        ScalarValue::Typed(crate::types::typed::TypedValue::Numeric(numeric)) => numeric
            .to_i64_rounded()
            .map_err(|error| QueryError::Cassie(crate::app::CassieError::TypeInput(error)))?
            .ok_or_else(|| range(width))?,
        ScalarValue::Typed(typed) => {
            return match typed.as_f64() {
                Some(float) => cast(&ScalarValue::Float(float), width, controls),
                None => Err(QueryError::General(width.syntax_message().into())),
            }
        }
        // Preserve Cassie's existing decimal-only JSON scalar conversion.
        // PostgreSQL text integer grammar must not admit new JSON spellings.
        ScalarValue::Json(input) => input.parse::<i64>().map_err(|error| {
            if matches!(
                error.kind(),
                std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
            ) {
                range(width)
            } else {
                QueryError::General(width.syntax_message().into())
            }
        })?,
        ScalarValue::Null => return Ok(ScalarValue::Null),
        ScalarValue::Float(_) => return Err(range(width)),
    };
    if !width.contains(value) {
        return Err(range(width));
    }
    Ok(ScalarValue::Int(value))
}

pub(super) fn arithmetic(
    left: &ScalarValue,
    operation: IntegerOperation,
    width: IntegerWidth,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let (ScalarValue::Int(left), ScalarValue::Int(right)) = (left, right) else {
        return Err(QueryError::General(
            "integer operator requires integer operands".into(),
        ));
    };
    integer::arithmetic(width, operation, *left, *right)
        .map(ScalarValue::Int)
        .map_err(|error| match error {
            IntegerArithmeticError::Range => range(width),
            IntegerArithmeticError::DivisionByZero => {
                QueryError::General("division by zero".into())
            }
        })
}

fn range(width: IntegerWidth) -> QueryError {
    QueryError::General(width.range_message().into())
}

pub(super) fn negate(value: &ScalarValue, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Null => Ok(ScalarValue::Null),
        ScalarValue::Int(value) => integer::negate(width, *value)
            .map(ScalarValue::Int)
            .map_err(|_| range(width)),
        _ => Err(QueryError::General(
            "integer negation requires an integer operand".into(),
        )),
    }
}

#[cfg(test)]
pub(super) fn input(input: &str, width: IntegerWidth) -> Result<ScalarValue, QueryError> {
    controlled_input(input, width, None)
}

pub(super) fn controlled_input(
    input: &str,
    width: IntegerWidth,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    let value = match integer::parse(input, width) {
        Ok(value) => return Ok(ScalarValue::Int(value)),
        Err(error) => error,
    };
    let name = match width {
        IntegerWidth::I16 => "smallint",
        IntegerWidth::I32 => "integer",
        IntegerWidth::I64 => "bigint",
    };
    let diagnostic = match value {
        IntegerInputError::Syntax => ScalarLiteralDiagnostic::Syntax(name),
        IntegerInputError::Range => ScalarLiteralDiagnostic::Range(name),
    };
    let capacity = diagnostic.capacity(input).ok_or_else(|| {
        QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            "integer diagnostic capacity overflow".into(),
        ))
    })?;
    let mut reservation = controls
        .map(|controls| controls.reserve_query_memory(capacity))
        .transpose()?;
    let mut message = String::new();
    message.try_reserve_exact(capacity).map_err(|error| {
        QueryError::Cassie(crate::app::CassieError::ResourceLimit(format!(
            "unable to allocate integer input diagnostic: {error}"
        )))
    })?;
    if let Some(reservation) = &mut reservation {
        if message.capacity() > capacity {
            reservation.try_grow(message.capacity() - capacity)?;
        }
    }
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    diagnostic
        .write(input, &mut message)
        .expect("writing an integer input diagnostic");
    Err(QueryError::General(message))
}

#[cfg(test)]
mod review_tests {
    use super::{cast, input, IntegerWidth, ScalarValue};

    #[test]
    fn should_preserve_runtime_integer_input_detail() {
        // Arrange
        let cases = [
            (
                IntegerWidth::I16,
                "x",
                "invalid input syntax for type smallint: \"x\"",
            ),
            (
                IntegerWidth::I32,
                "x",
                "invalid input syntax for type integer: \"x\"",
            ),
            (
                IntegerWidth::I64,
                "x",
                "invalid input syntax for type bigint: \"x\"",
            ),
            (
                IntegerWidth::I16,
                "32768",
                "value \"32768\" is out of range for type smallint",
            ),
            (
                IntegerWidth::I32,
                "99999999999",
                "value \"99999999999\" is out of range for type integer",
            ),
            (
                IntegerWidth::I64,
                "9223372036854775808",
                "value \"9223372036854775808\" is out of range for type bigint",
            ),
        ];
        // Act
        let errors = cases.map(|(width, text, _)| {
            (
                cast(&ScalarValue::Str(text.to_owned()), width, None)
                    .expect_err("invalid cast")
                    .to_string(),
                input(text, width).expect_err("invalid input").to_string(),
            )
        });
        // Assert
        for ((cast, input), (_, _, expected)) in errors.into_iter().zip(cases) {
            assert_eq!(cast, expected);
            assert_eq!(input, expected);
        }
    }
    #[test]
    fn should_admit_runtime_integer_diagnostics_before_copying_input() {
        // Arrange
        use crate::app::CassieError;
        use crate::config::CassieRuntimeLimits;
        use crate::runtime::QueryExecutionControls;
        let input = "xxxxxxxxxxxxxxxx";
        let expected = "invalid input syntax for type integer: \"xxxxxxxxxxxxxxxx\"";
        let controls = |budget| {
            QueryExecutionControls::from_limits(
                &CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..CassieRuntimeLimits::default()
                },
                std::time::Instant::now(),
            )
        };
        let exact = controls(expected.len());
        let below = controls(expected.len() - 1);
        // Act
        let admitted = super::controlled_input(input, IntegerWidth::I32, Some(&exact));
        let rejected = super::controlled_input(input, IntegerWidth::I32, Some(&below));
        let row = vec![(
            "payload".to_owned(),
            super::super::Value::String(input.into()),
        )];
        let expression = super::super::Expr::Cast {
            expr: Box::new(super::super::Expr::Column("payload".into())),
            data_type: crate::types::DataType::Int,
            source_type: Some(crate::types::DataType::Text),
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
        let functions = std::collections::HashMap::new();
        let runtime = super::super::eval_scalar_with_context(
            &row,
            &expression,
            super::super::EvalContext {
                params: &[],
                local_args: None,
                user_functions: &functions,
                exists: None,
                session: None,
                search_context: None,
                controls: Some(&below),
            },
        );
        let constant = super::super::constants::value(
            &super::super::Expr::Cast {
                expr: Box::new(super::super::Expr::StringLiteral(input.into())),
                data_type: crate::types::DataType::Int,
                source_type: Some(crate::types::DataType::Text),
                identity: crate::sql::ast::CastIdentity::Scalar,
            },
            &row,
            super::super::EvalContext {
                params: &[],
                local_args: None,
                user_functions: &functions,
                exists: None,
                session: None,
                search_context: None,
                controls: Some(&below),
            },
        );
        // Assert
        assert!(matches!(
            constant,
            Err(super::QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(admitted.expect_err("invalid integer").to_string(), expected);
        assert_eq!(exact.peak_query_memory_bytes(), expected.len());
        assert_eq!(exact.current_query_memory_bytes(), 0);
        assert!(matches!(
            rejected,
            Err(super::QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert!(matches!(
            runtime,
            Err(super::QueryError::Cassie(CassieError::ResourceLimit(_)))
        ));
        assert_eq!(below.peak_query_memory_bytes(), 0);
        assert_eq!(below.current_query_memory_bytes(), 0);
    }
}
