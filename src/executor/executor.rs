use crate::app::Cassie;
use crate::executor::{aggregate, filter, projection, scan, sort};
use crate::planner::physical::PhysicalPlan;
use crate::types::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
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

pub async fn run(
    cassie: &Cassie,
    plan: PhysicalPlan,
    params: Vec<Value>,
) -> Result<QueryResult, QueryError> {
    let mut rows = scan::scan(cassie, &plan.logical.collection).await?;
    let text_fields = cassie.catalog.text_fields(&plan.logical.collection).await;
    let mut field_boost: HashMap<String, f64> = HashMap::new();
    for field in &text_fields {
        if let Some(boost) = cassie
            .catalog
            .get_field_boost(&plan.logical.collection, field)
            .await
        {
            field_boost.insert(field.clone(), boost as f64);
        }
    }
    let search_context = filter::SearchContext::from_rows(&rows, &text_fields, &field_boost);

    if let Some(filter_expr) = &plan.logical.filter {
        rows = filter::filter_rows(rows, filter_expr, &params, Some(&search_context))?;
    }

    if !plan.logical.order.is_empty() {
        rows = sort::sort_rows(
            rows,
            &plan.logical.order,
            &plan.logical.projection,
            &params,
            Some(&search_context),
        )?;
    }

    let mut rows = projection::project(
        rows,
        &plan.logical.projection,
        &params,
        Some(&search_context),
    )?;

    if let Some(offset) = plan.logical.offset {
        let off = offset.max(0) as usize;
        rows = rows.into_iter().skip(off).collect();
    }

    if let Some(limit) = plan.logical.limit {
        rows = rows.into_iter().take(limit.max(0) as usize).collect();
    }

    let columns = aggregate::columns_from_projection(&plan.logical.projection);
    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}
