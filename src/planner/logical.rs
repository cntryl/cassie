use crate::app::CassieError;
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

pub fn plan(bound: &BoundStatement) -> Result<LogicalPlan, CassieError> {
    let crate::sql::ast::QueryStatement::Select(select) = &bound.statement.statement;
    validate_logical_plan(select)?;

    Ok(LogicalPlan {
        collection: select.collection.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
    })
}

fn validate_logical_plan(select: &crate::sql::ast::SelectStatement) -> Result<(), CassieError> {
    if select.collection.trim().is_empty() {
        return Err(CassieError::Planner(
            "planner cannot build plan for empty collection name".to_string(),
        ));
    }

    if select.projection.is_empty() {
        return Err(CassieError::Planner(
            "planner cannot build plan with empty projection".to_string(),
        ));
    }

    if let Some(limit) = select.limit {
        if limit < 0 {
            return Err(CassieError::Planner(format!(
                "planner cannot build plan with negative limit: {limit}"
            )));
        }
    }

    if let Some(offset) = select.offset {
        if offset < 0 {
            return Err(CassieError::Planner(format!(
                "planner cannot build plan with negative offset: {offset}"
            )));
        }
    }

    Ok(())
}
