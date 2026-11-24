//! Operator selection precedes coercion to the pagination integer domain.
use crate::app::CassieError;
use crate::sql::{ast::Expr, binder, FieldTypeMap};

pub(super) fn validate(expression: &Expr) -> Result<(), CassieError> {
    validate_at_depth(expression, 0)
}

fn validate_at_depth(expression: &Expr, depth: usize) -> Result<(), CassieError> {
    if depth > 128 {
        return Err(CassieError::ResourceLimit(
            "pagination plan nesting exceeds 128 expression levels".into(),
        ));
    }
    expression.try_visit_children(&mut |child| validate_at_depth(child, depth + 1))?;
    if let Expr::UnaryNumeric { expr, op } = expression {
        let data_type = binder::known_expr_type(expr, &FieldTypeMap::new());
        binder::unary_numeric::validate_signature(*op, expr, data_type.as_ref())?;
    }
    Ok(())
}
