//! Existing JSON key and path operator operand families.
use super::{BinaryOp, CassieError, DataType, Expr, OperandFamily};

pub(super) fn json_delete_key_family(
    right: &Expr,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    validate_json_delete_key_operand(right, right_family, field_types)?;
    Ok(Some(OperandFamily::Json))
}

fn validate_json_delete_key_operand(
    right: &Expr,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    if matches!(right, Expr::StringLiteral(_) | Expr::Null | Expr::Param(_))
        || right_family == Some(OperandFamily::Text)
    {
        return Ok(());
    }
    let right_type = super::super::known_expr_type(right, field_types);
    let is_integer = right_type
        .as_ref()
        .is_some_and(|data_type| matches!(data_type, DataType::SmallInt | DataType::Int));
    let is_text_array = right_type.as_ref().is_some_and(
        |data_type| matches!(data_type, DataType::Array(element) if **element == DataType::Text),
    );
    if is_integer || is_text_array {
        return Ok(());
    }
    let right_type = right_type.map_or_else(
        || right_family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
        |data_type| data_type.type_name(),
    );
    Err(CassieError::Execution(format!(
        "operator does not exist: json - {right_type}"
    )))
}

pub(super) fn json_delete_path_family(
    left: &Expr,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    if left_family.is_some_and(|family| family != OperandFamily::Json) {
        return Err(CassieError::Execution(
            "operator does not exist: non-json #- text[]".into(),
        ));
    }
    let right_type = super::super::known_expr_type(right, field_types);
    if matches!(right, Expr::Null | Expr::Param(_))
        || right_type.as_ref().is_some_and(|data_type| {
            matches!(data_type, DataType::Array(element) if **element == DataType::Text)
        })
    {
        return Ok(Some(OperandFamily::Json));
    }
    let left_type = super::super::known_expr_type(left, field_types)
        .map_or_else(|| "unknown".to_owned(), |data_type| data_type.type_name());
    let right_type = right_type.map_or_else(
        || right_family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
        |data_type| data_type.type_name(),
    );
    Err(CassieError::Execution(format!(
        "operator does not exist: {left_type} #- {right_type}"
    )))
}

pub(super) fn json_key_operator_family(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    families: (Option<OperandFamily>, Option<OperandFamily>),
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    validate_json_key_operator_operands(left, op, right, families.0, families.1, field_types)?;
    Ok(Some(OperandFamily::Boolean))
}

fn validate_json_key_operator_operands(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    let operator = match op {
        BinaryOp::JsonbKeyExists => "?",
        BinaryOp::JsonbKeyExistsAny => "?|",
        BinaryOp::JsonbKeyExistsAll => "?&",
        _ => return Ok(()),
    };
    if left_family.is_some_and(|family| family != OperandFamily::Json) {
        return Err(json_key_operator_error(
            left,
            operator,
            right,
            left_family,
            right_family,
            field_types,
        ));
    }
    if matches!(op, BinaryOp::JsonbKeyExists) {
        if !matches!(right, Expr::StringLiteral(_) | Expr::Null | Expr::Param(_))
            && right_family.is_some_and(|family| family != OperandFamily::Text)
        {
            return Err(json_key_operator_error(
                left,
                operator,
                right,
                left_family,
                right_family,
                field_types,
            ));
        }
        return Ok(());
    }
    let right_type = super::super::known_expr_type(right, field_types);
    match (right, right_type) {
        (Expr::StringLiteral(_) | Expr::Null | Expr::Param(_), _) => Ok(()),
        (_, Some(DataType::Array(element))) if *element == DataType::Text => Ok(()),
        (_, Some(DataType::Null) | None) if right_family.is_none() => Ok(()),
        _ => Err(json_key_operator_error(
            left,
            operator,
            right,
            left_family,
            right_family,
            field_types,
        )),
    }
}

fn json_key_operator_error(
    left: &Expr,
    operator: &str,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> CassieError {
    let type_name = |expr: &Expr, family: Option<OperandFamily>| {
        super::super::known_expr_type(expr, field_types).map_or_else(
            || family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
            |data_type| data_type.type_name(),
        )
    };
    CassieError::Execution(format!(
        "operator does not exist: {} {operator} {}",
        type_name(left, left_family),
        type_name(right, right_family)
    ))
}
