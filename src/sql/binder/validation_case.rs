use super::{
    expression_operand_family, require_compatible_families, require_family, validate_expression,
    CassieError, DataType, Expr, HashSet, OperandFamily,
};

pub(super) fn case_operand_family(
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    let operand_family = operand
        .map(|value| expression_operand_family(value, field_types))
        .transpose()?
        .flatten();
    let mut result_family = None;
    let mut result_type = DataType::Null;
    for (when, then) in branches {
        let when_family = expression_operand_family(when, field_types)?;
        if operand.is_some() {
            require_compatible_families(operand_family, when_family, "CASE WHEN")?;
        } else {
            require_family(when_family, OperandFamily::Boolean, "CASE WHEN")?;
        }
        let then_family = expression_operand_family(then, field_types)?;
        require_compatible_families(result_family, then_family, "CASE result")?;
        result_family = result_family.or(then_family);
        if let Some(branch_type) = known_case_result_type(then, field_types) {
            result_type = super::super::inference::common_case_type(result_type, branch_type)
                .ok_or_else(|| CassieError::Planner("incompatible CASE result types".into()))?;
        }
    }
    if let Some(else_expr) = else_expr {
        let else_family = expression_operand_family(else_expr, field_types)?;
        require_compatible_families(result_family, else_family, "CASE result")?;
        result_family = result_family.or(else_family);
        if let Some(branch_type) = known_case_result_type(else_expr, field_types) {
            let _ = super::super::inference::common_case_type(result_type, branch_type)
                .ok_or_else(|| CassieError::Planner("incompatible CASE result types".into()))?;
        }
    }
    Ok(result_family)
}

fn known_case_result_type(expr: &Expr, field_types: &crate::sql::FieldTypeMap) -> Option<DataType> {
    match expr {
        Expr::Column(name) => crate::sql::field_type_for_column(field_types, name).cloned(),
        Expr::Cast { data_type, .. } => Some(data_type.clone()),
        Expr::StringLiteral(_) => Some(DataType::Text),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::IntegerLiteral(_) => Some(DataType::BigInt),
        Expr::BoolLiteral(_) => Some(DataType::Boolean),
        Expr::Null => Some(DataType::Null),
        _ => None,
    }
}

pub(super) fn validate_case_references(
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    if let Some(operand) = operand {
        validate_expression(
            operand,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        )?;
    }
    for (when, then) in branches {
        validate_expression(
            when,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        )?;
        validate_expression(
            then,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        )?;
    }
    if let Some(else_expr) = else_expr {
        validate_expression(
            else_expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        )?;
    }
    Ok(())
}
