//! A parameter compared with a subquery's output takes that column's type,
//! as in `$1 IN (SELECT n FROM t)`.
use super::{
    infer_parameter_type_from_expected_expr, infer_parameter_type_oids_expr,
    infer_parameter_type_oids_query, ParameterInference,
};
use crate::sql::ast::{QueryStatement, SubqueryExpr};

/// Infers a subquery's own parameters, then a sublink's outer operands'.
pub(super) fn infer(
    expr: &crate::sql::ast::Expr,
    field_types: &crate::sql::FieldTypeMap,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let subquery = match expr {
        crate::sql::ast::Expr::Exists(statement) => {
            return infer_parameter_type_oids_query(&statement.statement, catalog, oids);
        }
        crate::sql::ast::Expr::Subquery(subquery) => subquery,
        _ => return,
    };
    infer_parameter_type_oids_query(&subquery.query.statement, catalog, oids);
    infer_operands(subquery, catalog, oids);
    for operand in subquery.operands() {
        infer_parameter_type_oids_expr(operand, field_types, catalog, oids);
    }
}

fn infer_operands(
    subquery: &SubqueryExpr,
    catalog: &crate::catalog::Catalog,
    oids: &mut ParameterInference,
) {
    let operands = subquery.operands();
    if operands.is_empty() || !operands.iter().any(has_parameter) {
        return;
    }
    let QueryStatement::Select(select) = &subquery.query.statement else {
        return;
    };
    let Ok(schema) = crate::sql::binder::infer_select_schema(select, catalog) else {
        return;
    };
    for (operand, field) in operands.iter().zip(&schema.fields) {
        infer_parameter_type_from_expected_expr(operand, &field.data_type, oids);
    }
}

fn has_parameter(expr: &crate::sql::ast::Expr) -> bool {
    expr.any_descendant_or_self(&mut |expr| matches!(expr, crate::sql::ast::Expr::Param(_)))
}
