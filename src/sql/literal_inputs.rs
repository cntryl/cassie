//! Borrowed unknown-literal conversion checks during binding.
use crate::app::CassieError;
use crate::sql::ast::{CastIdentity, Expr};
use crate::types::{DataType, IntegerWidth};

pub(crate) fn validate(
    expression: &Expr,
    target: &DataType,
    identity: CastIdentity,
) -> Result<(), CassieError> {
    let Expr::StringLiteral(input) = expression else {
        return Ok(());
    };
    // NUMERIC has its own input law; its historical FLOAT carrier is not proof
    // that this conversion invokes float8in.
    if identity != CastIdentity::Scalar {
        return Ok(());
    }
    if let Some(width) = IntegerWidth::from_data_type(target) {
        return crate::types::integer::parse(input, width)
            .map(|_| ())
            .map_err(|error| {
                use crate::types::integer::IntegerInputError;
                CassieError::Execution(
                    match error {
                        IntegerInputError::Syntax => width.syntax_message(),
                        IntegerInputError::Range => width.range_message(),
                    }
                    .into(),
                )
            });
    }
    match target {
        DataType::Float => crate::types::float_input::parse(input)
            .map(|_| ())
            .map_err(CassieError::from),
        DataType::Boolean => crate::types::boolean::parse_text(input)
            .map(|_| ())
            .ok_or_else(|| CassieError::from(crate::types::BooleanInputError)),
        _ => Ok(()),
    }
}
