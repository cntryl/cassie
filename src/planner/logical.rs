use crate::app::CassieError;
use crate::sql::{
    ast::{CommonTableExpression, Expr, OrderExpr, QuerySource},
    binder::BoundStatement,
};

#[derive(Debug, Clone)]
pub struct LogicalPlan {
    pub source: QuerySource,
    pub collection: String,
    pub ctes: Vec<CommonTableExpression>,
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
        source: select.source.clone(),
        collection: source_name(&select.source),
        ctes: select.ctes.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
    })
}

fn source_name(source: &QuerySource) -> String {
    match source {
        QuerySource::Collection(name) | QuerySource::Cte(name) => name.clone(),
    }
}

fn validate_logical_plan(select: &crate::sql::ast::SelectStatement) -> Result<(), CassieError> {
    if source_name(&select.source).trim().is_empty() {
        return Err(CassieError::Planner(
            "planner cannot build plan for empty source name".to_string(),
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
