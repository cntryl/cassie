use super::{
    aggregate, BatchRow, Cassie, CmpOrdering, FunctionMeta, HashMap, HashSet, PhysicalPlan,
    QueryError, QueryExecutionControls, QueryResult, Value,
};
use crate::executor::batch::RowAccess;
use crate::executor::semantic::{compare_values, SemanticKey};

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
        .or_else(|| cassie.catalog.get_schema(&plan.logical.collection))
        // A CTE is not a catalog object, so without this its columns would all
        // be reported as text.
        .or_else(|| {
            crate::sql::binder::cte_collection_schema(
                &plan.logical.ctes,
                &plan.logical.collection,
                &cassie.catalog,
            )
        });
    let columns = aggregate::columns_from_projection(
        &plan.logical.projection,
        collection_schema.as_ref(),
        user_functions,
    );
    // Every row carries the reserved `_id` internal-identity entry (see
    // `scan::push_row_identity`) as working state for DML/retention/scored-
    // candidate resolution; it is never a `SELECT` output column, so it's
    // dropped here rather than earlier, to keep it available to every
    // internal consumer up to this final boundary. `SELECT *` against a
    // table with no declared `id` field is the one exception: its `id`
    // output column *is* that same internal identity (see
    // `aggregate::columns_from_projection`'s wildcard branch and
    // `scan::push_row_identity`'s doc comment), and rather than pay for a
    // second physical copy of the value in every row, that single `_id`
    // entry is kept instead of dropped, landing in the `id` column's
    // position because both list it first.
    let keep_internal_identity_as_id = plan
        .logical
        .projection
        .iter()
        .any(|item| matches!(item, crate::sql::ast::SelectItem::Wildcard))
        && !collection_schema
            .as_ref()
            .is_some_and(crate::catalog::CollectionSchema::declares_id);
    // A query can also name `_id` as an output column of its own, either by
    // selecting it directly or by aliasing an expression to it. Dropping the
    // matching entry would emit a row narrower than its own `RowDescription`
    // — a protocol violation, not just a missing value — so the entry is
    // kept whenever a column claims that name.
    let projects_internal_identity = columns
        .iter()
        .any(|column| crate::types::row_identity::is_row_identity_column(&column.name));
    let keep_internal_identity_as_id = keep_internal_identity_as_id || projects_internal_identity;
    let rows: Vec<Vec<Value>> = rows
        .into_iter()
        .map(|row| {
            row.into_entries()
                .into_iter()
                .filter(|(name, _)| {
                    keep_internal_identity_as_id
                        || !crate::types::row_identity::is_row_identity_column(name)
                })
                .map(|(_, value)| value)
                .collect()
        })
        .collect();

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
    compare_values(left, right)
}

pub(super) fn row_signature(row: &impl RowAccess) -> SemanticKey {
    SemanticKey::from_values(row.entries().iter().map(|(_, value)| value))
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
