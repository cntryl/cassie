//! Resolve selected text concatenation without guessing shared carriers.
use crate::app::CassieError;
use crate::types::DataType;

pub(super) fn resolve(
    left: Option<&DataType>,
    right: Option<&DataType>,
) -> Result<(), CassieError> {
    let text = |kind: &DataType| {
        matches!(
            kind,
            DataType::Text | DataType::Varchar { .. } | DataType::Null
        )
    };
    let supported = |kind: &DataType| {
        text(kind)
            || matches!(
                kind,
                DataType::SmallInt
                    | DataType::Int
                    | DataType::BigInt
                    | DataType::Float
                    | DataType::Boolean
            )
    };
    if [left, right]
        .into_iter()
        .flatten()
        .any(|kind| !supported(kind))
    {
        return Err(CassieError::Unsupported(
            "concatenation for this input layout remains pending under #793/#796".into(),
        ));
    }
    if left.is_some_and(|kind| !text(kind)) && right.is_some_and(|kind| !text(kind)) {
        return Err(CassieError::Execution(
            "operator does not exist: non-text || non-text".into(),
        ));
    }
    Ok(())
}
