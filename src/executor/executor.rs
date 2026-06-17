use crate::app::Cassie;
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::planner::logical::LogicalPlan;
use crate::planner::physical::PhysicalPlan;
use crate::sql::ast::{CteQuery, CommonTableExpression, QuerySource};
use crate::types::Value;
use std::future::Future;
use std::collections::{HashMap, HashSet};
use std::pin::Pin;

const MAX_RECURSIVE_CTE_DEPTH: usize = 64;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
}

#[derive(Debug)]
pub enum QueryError {
    General(String),
}

type CteRows = Vec<Vec<(String, Value)>>;
type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Pin<Box<dyn Future<Output = Result<CteRows, QueryError>> + Send + 'a>>;

pub async fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let mut cte_context: CteContext = HashMap::new();
    let rows = execute_plan(cassie, &plan.logical, &mut cte_context, &params).await?;

    let columns = aggregate::columns_from_projection(&plan.logical.projection);
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(|(_, value)| value).collect())
        .collect();

    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}

async fn execute_plan(
    cassie: &Cassie,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    params: &[Value],
) -> Result<CteRows, QueryError> {
    for cte in &plan.ctes {
        let rows = execute_cte(cassie, cte, cte_context, params).await?;
        cte_context.insert(cte.name.to_ascii_lowercase(), rows);
    }

    execute_source_query(cassie, plan, cte_context, params).await
}

fn execute_cte<'a>(
    cassie: &'a Cassie,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
    params: &'a [Value],
) -> CteExecution<'a> {
    Box::pin(async move {
        let cte_name = cte.name.to_ascii_lowercase();
        let previous = cte_context.remove(&cte_name);

        let output = match &cte.query {
            CteQuery::Simple(statement) => {
                let logical = build_logical_plan(statement.as_ref())?;
                execute_plan(cassie, &logical, cte_context, params).await?
            }
            CteQuery::Recursive { base, recursive } => {
                let base_plan = build_logical_plan(base.as_ref())?;
                let mut rows = execute_plan(cassie, &base_plan, cte_context, params).await?;

                cte_context.insert(cte_name.clone(), rows.clone());

                let mut seen: HashSet<String> = rows.iter().map(row_signature).collect();
                let mut stabilized = false;

                for _ in 0..MAX_RECURSIVE_CTE_DEPTH {
                    let recursive_plan = build_logical_plan(recursive.as_ref())?;
                    let recursive_rows = execute_plan(cassie, &recursive_plan, cte_context, params).await?;

                    let mut new_rows = Vec::new();
                    for row in recursive_rows {
                        let signature = row_signature(&row);
                        if seen.insert(signature) {
                            rows.push(row.clone());
                            new_rows.push(row);
                        }
                    }

                    if new_rows.is_empty() {
                        stabilized = true;
                        break;
                    }

                    cte_context.insert(cte_name.clone(), rows.clone());
                }

                if !stabilized {
                    return Err(QueryError::General(format!(
                        "recursive CTE '{}' did not stabilize within {} iterations",
                        cte.name, MAX_RECURSIVE_CTE_DEPTH
                    )));
                }

                rows
            }
        };

        if let Some(previous_rows) = previous {
            cte_context.insert(cte_name, previous_rows);
        } else {
            cte_context.remove(&cte_name);
        }

        Ok(output)
    })
}

fn build_logical_plan(statement: &crate::sql::ast::ParsedStatement) -> Result<LogicalPlan, QueryError> {
    crate::planner::logical::plan(&crate::sql::binder::BoundStatement {
        statement: statement.clone(),
    })
    .map_err(|error| QueryError::General(error.to_string()))
}

async fn execute_source_query(
    cassie: &Cassie,
    plan: &LogicalPlan,
    cte_context: &mut CteContext,
    params: &[Value],
) -> Result<CteRows, QueryError> {
    let (mut rows, text_fields) = match &plan.source {
        QuerySource::Collection(name) => {
            let rows = scan::scan(cassie, name).await?;
            (rows, cassie.catalog.text_fields(name).await)
        }
        QuerySource::Cte(name) => {
            let key = name.to_ascii_lowercase();
            let rows = cte_context
                .get(&key)
                .cloned()
                .ok_or_else(|| QueryError::General(format!("relation '{name}' does not exist")))?;
            let text_fields = deduce_text_fields(&rows);
            (rows, text_fields)
        }
    };

    let field_boost = if let QuerySource::Collection(name) = &plan.source {
        let fields = cassie.catalog.text_fields(name).await;
        let mut boost = HashMap::with_capacity(fields.len());
        for field in fields {
            if let Some(value) = cassie.catalog.get_field_boost(name, &field).await {
                boost.insert(field, value as f64);
            }
        }
        boost
    } else {
        HashMap::new()
    };

    let search_context = filter::SearchContext::from_rows(&rows, &text_fields, &field_boost);

    if let Some(filter_expr) = &plan.filter {
        rows = filter::filter_rows(rows, filter_expr, params, Some(&search_context))?;
    }

    if !plan.order.is_empty() {
        rows = sort::sort_rows(
            rows,
            &plan.order,
            &plan.projection,
            params,
            Some(&search_context),
        )?;
    }

    rows = projection::project_rows(rows, &plan.projection, params, Some(&search_context))?;

    if let Some(offset) = plan.offset {
        let offset = offset.max(0) as usize;
        rows = rows.into_iter().skip(offset).collect();
    }

    if let Some(limit) = plan.limit {
        let limit = limit.max(0) as usize;
        rows = rows.into_iter().take(limit).collect();
    }

    Ok(rows)
}

fn row_signature(row: &Vec<(String, Value)>) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| String::new())
}

fn deduce_text_fields(rows: &[Vec<(String, Value)>]) -> Vec<String> {
    let mut fields = HashSet::<String>::new();
    let mut ordered = Vec::new();

    for row in rows {
        for (name, value) in row {
            if !matches!(value, Value::String(_) | Value::Json(_)) {
                continue;
            }

            let name = name.to_ascii_lowercase();
            if fields.insert(name.clone()) {
                ordered.push(name);
            }
        }
    }

    ordered
}
