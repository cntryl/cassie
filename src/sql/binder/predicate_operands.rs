use super::{
    boolean_aggregates, validate_expression, BindingContext, CassieError, Catalog, DataType, Expr,
    ResultTypes,
};

pub(super) fn validate_truth_operand(
    expr: &mut Expr,
    truth: crate::sql::ast::TruthValue,
    negated: bool,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if let Expr::StringLiteral(text) = expr {
        let value = crate::types::boolean::parse_text(text).ok_or_else(|| {
            CassieError::Execution(format!("invalid input syntax for type boolean: {text:?}"))
        })?;
        *expr = Expr::BoolLiteral(value);
    }
    if let Some(kind) = types.expression_type(expr) {
        if !matches!(kind, DataType::Boolean | DataType::Null) {
            return Err(CassieError::Execution(format!(
                "argument of {} must be type boolean, not type {}",
                truth.label(negated),
                boolean_aggregates::type_name(&kind)
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_like_expression(
    expression: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Expr::Like {
        expr,
        pattern,
        escape,
        ..
    } = expression
    else {
        unreachable!("LIKE validation dispatch")
    };
    for operand in [&mut **expr, &mut **pattern, &mut **escape] {
        validate_expression(operand, types, catalog, context)?;
    }
    super::super::like_predicates::validate(expr, pattern, escape, types)
}
