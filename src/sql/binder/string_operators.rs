//! Resolve text and BYTEA concatenation before their shared carriers are evaluated.
use crate::app::CassieError;
use crate::sql::ast::{BinaryOp, CastIdentity, Expr};
use crate::types::DataType;

pub(super) fn resolve(
    op: &mut BinaryOp,
    left: Option<&DataType>,
    right: Option<&DataType>,
) -> Result<(), CassieError> {
    if *op == BinaryOp::ByteaConcat {
        if [left, right]
            .into_iter()
            .flatten()
            .any(|kind| !matches!(kind, DataType::Bytea | DataType::Null))
        {
            return Err(CassieError::Planner(
                "resolved BYTEA concatenation does not match its operand types".into(),
            ));
        }
        return Ok(());
    }
    if (left == Some(&DataType::Bytea) || right == Some(&DataType::Bytea))
        && [left, right]
            .into_iter()
            .flatten()
            .all(|kind| matches!(kind, DataType::Bytea | DataType::Null))
    {
        *op = BinaryOp::ByteaConcat;
        return Ok(());
    }
    if left == Some(&DataType::Json) || right == Some(&DataType::Json) {
        if [left, right]
            .into_iter()
            .flatten()
            .all(|kind| matches!(kind, DataType::Json | DataType::Null))
        {
            return Ok(());
        }
        return Err(CassieError::Execution(
            "operator does not exist: jsonb || incompatible operands".into(),
        ));
    }
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

/// Keep unknown literal origin until the BYTEA versus text overload is chosen.
pub(super) fn qualify(
    left: &mut Expr,
    op: &BinaryOp,
    right: &mut Expr,
    left_type: Option<&DataType>,
    right_type: Option<&DataType>,
) {
    if *op != BinaryOp::Concat {
        return;
    }
    let json_left = left_type == Some(&DataType::Json);
    let json_right = right_type == Some(&DataType::Json);
    let bytea_left = left_type == Some(&DataType::Bytea);
    let bytea_right = right_type == Some(&DataType::Bytea);
    if json_left && matches!(right, Expr::StringLiteral(_)) {
        cast(right, DataType::Json, None);
    } else if json_right && matches!(left, Expr::StringLiteral(_)) {
        cast(left, DataType::Json, None);
    }
    if json_left && !is_json_cast(left) {
        cast(left, DataType::Json, Some(DataType::Json));
    }
    if json_right && !is_json_cast(right) {
        cast(right, DataType::Json, Some(DataType::Json));
    }
    if !json_left && !json_right && bytea_left && matches!(right, Expr::StringLiteral(_)) {
        cast(right, DataType::Bytea, None);
    } else if !json_left && !json_right && bytea_right && matches!(left, Expr::StringLiteral(_)) {
        cast(left, DataType::Bytea, None);
    } else if !json_left
        && !json_right
        && bytea_left
        && matches!(right_type, Some(DataType::Text | DataType::Varchar { .. }))
    {
        cast(left, DataType::Text, Some(DataType::Bytea));
    } else if !json_left
        && !json_right
        && bytea_right
        && matches!(left_type, Some(DataType::Text | DataType::Varchar { .. }))
    {
        cast(right, DataType::Text, Some(DataType::Bytea));
    }
}

fn is_json_cast(expression: &Expr) -> bool {
    matches!(
        expression,
        Expr::Cast {
            data_type: DataType::Json,
            ..
        }
    )
}

fn cast(expression: &mut Expr, data_type: DataType, source_type: Option<DataType>) {
    *expression = Expr::Cast {
        expr: Box::new(std::mem::replace(expression, Expr::Null)),
        data_type,
        source_type,
        identity: CastIdentity::Scalar,
    };
}

#[cfg(test)]
mod tests {
    use super::{resolve, BinaryOp, DataType};
    #[test]
    fn should_preserve_legacy_concat_given_resolved_bytea_mode_when_operator_metadata_is_serialized(
    ) {
        // Arrange
        let legacy = "\"Concat\"";
        // Act
        let old: BinaryOp = serde_json::from_str(legacy).unwrap();
        let encoded = serde_json::to_string(&BinaryOp::ByteaConcat).unwrap();
        let new: BinaryOp = serde_json::from_str(&encoded).unwrap();
        // Assert
        assert_eq!(old, BinaryOp::Concat);
        assert_eq!(serde_json::to_string(&old).unwrap(), legacy);
        assert_eq!(new, BinaryOp::ByteaConcat);
    }
    #[test]
    fn should_reject_mismatched_types_given_resolved_bytea_mode_when_binding_is_revalidated() {
        // Arrange
        let mut op = BinaryOp::ByteaConcat;
        // Act
        let error = resolve(&mut op, Some(&DataType::Bytea), Some(&DataType::Text));
        // Assert
        assert!(error.is_err());
        assert_eq!(op, BinaryOp::ByteaConcat);
    }
}
