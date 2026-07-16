use super::{
    aggregate, BatchRow, Cassie, CmpOrdering, FunctionMeta, HashMap, HashSet, PhysicalPlan,
    QueryError, QueryExecutionControls, QueryResult, Value,
};
use crate::executor::batch::RowAccess;

pub(super) fn build_select_result(
    cassie: &Cassie,
    plan: &PhysicalPlan,
    rows: Vec<BatchRow>,
    user_functions: &HashMap<String, FunctionMeta>,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let collection_schema = plan
        .collection_schema
        .clone()
        .or_else(|| cassie.catalog.get_schema(&plan.logical.collection));
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        user_functions,
    );
    let rows: Vec<Vec<Value>> = rows.into_iter().map(BatchRow::into_values).collect();

    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    Ok(QueryResult {
        columns,
        rows,
        command: "SELECT".to_string(),
    })
}

pub(super) fn compare_query_values(left: &Value, right: &Value) -> CmpOrdering {
    if let (Value::Int64(left), Value::Int64(right)) = (left, right) {
        return left.cmp(right);
    }
    if let (Some(left), Some(right)) = (left.as_f64(), right.as_f64()) {
        return left.partial_cmp(&right).unwrap_or(CmpOrdering::Equal);
    }
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return left.cmp(right);
    }
    CmpOrdering::Equal
}

pub(super) fn row_signature(row: &impl RowAccess) -> String {
    serde_json::to_string(row.entries()).unwrap_or_else(|_| String::new())
}

pub(super) fn deduce_text_fields<R: RowAccess>(rows: &[R]) -> Vec<String> {
    let mut fields = HashSet::<String>::new();
    let mut ordered = Vec::new();

    for row in rows {
        for (name, value) in row.entries() {
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
