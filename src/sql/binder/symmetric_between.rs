//! Keep newly admitted symmetric predicates inside selected scalar contracts.
use super::{coalesce_results::ResultTypes, CassieError, DataType, Expr};

pub(super) fn validate(
    operands: [&Expr; 3],
    symmetric: bool,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if symmetric
        && operands.into_iter().any(|operand| {
            matches!(
                types.expression_type(operand),
                Some(DataType::Array(_) | DataType::Json | DataType::Jsonb | DataType::Vector(_))
            )
        })
    {
        return Err(CassieError::Unsupported(
            "symmetric BETWEEN array, JSON and vector comparisons remain pending under #807/#809/#793".into(),
        ));
    }
    Ok(())
}
