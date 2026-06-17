use crate::sql::{
    ast::{Expr, OrderExpr},
    binder::BoundStatement,
};

#[derive(Debug, Clone)]
pub struct LogicalPlan {
    pub collection: String,
    pub projection: Vec<crate::sql::ast::SelectItem>,
    pub filter: Option<Expr>,
    pub order: Vec<OrderExpr>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub fn plan(bound: &BoundStatement) -> Result<LogicalPlan, crate::app::CassieError> {
    let select = match &bound.statement.statement {
        crate::sql::ast::QueryStatement::Select(sel) => sel,
    };
    Ok(LogicalPlan {
        collection: select.collection.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
    })
}
