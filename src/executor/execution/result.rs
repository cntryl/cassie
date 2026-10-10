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
    // A derived table or CTE is typed by its own body, never by a catalog
    // table that happens to share its alias.
    let collection_schema = crate::sql::binder::derived_source_schema(
        &plan.logical.source,
        &plan.logical.ctes,
        &cassie.catalog,
        user_functions,
    )
    .or_else(|| {
        crate::sql::binder::joined_source_schema(
            &plan.logical.source,
            &plan.logical.ctes,
            &cassie.catalog,
            user_functions,
        )
    })
    .or_else(|| plan.collection_schema.clone())
    .or_else(|| materialized_projection_schema(cassie, &plan.logical.collection))
    .or_else(|| cassie.catalog.get_schema(&plan.logical.collection))
    // Describe types built-in catalog views from their declared schema;
    // without the same arm here every such column executes as text and
    // the DataRow disagrees with the RowDescription.
    .or_else(|| crate::catalog::CollectionSchema::virtual_view(&plan.logical.collection))
    // A CTE is not a catalog object, so without this its columns would all
    // be reported as text.
    .or_else(|| {
        crate::sql::binder::cte_collection_schema_with_functions(
            &plan.logical.ctes,
            &plan.logical.collection,
            &cassie.catalog,
            user_functions,
        )
    });
    let wildcard_shape =
        aggregate::wildcard_shape_for_plan(&cassie.catalog, &plan.logical, user_functions);
    let wildcard_fields = wildcard_shape.as_ref().map(|(fields, _)| fields.as_slice());
    let columns = aggregate::columns_from_projection_with_wildcard(
        &plan.logical.projection,
        collection_schema.as_ref(),
        wildcard_fields,
        user_functions,
        &[],
    );
    // Derived wildcard schemas display physical `_id` as `id`, so
    // declares_id alone cannot distinguish it from a real declared field.
    // Retain only the identity that survives the same source-row hiding rule
    // used to infer these wildcard columns. Base tables keep their saved-plan
    // collection-schema rule when no derived wildcard shape is needed.
    let keep_internal_identity_as_id = wildcard_shape.as_ref().map_or_else(
        || {
            plan.logical
                .projection
                .iter()
                .any(|item| matches!(item, crate::sql::ast::SelectItem::Wildcard))
                && !collection_schema
                    .as_ref()
                    .is_some_and(crate::catalog::CollectionSchema::declares_id)
        },
        |(_, carries_identity)| *carries_identity,
    );
    // A query can also name `_id` as an output column of its own, either by
    // selecting it directly or by aliasing an expression to it. Dropping the
    // matching entry would emit a row narrower than its own `RowDescription`
    // — a protocol violation, not just a missing value — so the entry is
    // kept whenever a column claims that name.
    let projects_internal_identity = columns
        .iter()
        .any(|column| crate::types::row_identity::is_row_identity_column(&column.name));
    let keep_internal_identity_as_id = keep_internal_identity_as_id || projects_internal_identity;
    // Typed operators retain their parents until the complete public materialization boundary.
    // QueryResult owns its values independently; no private lease becomes part of its public API.
    let operator_rows = rows.iter().any(|row| row.operator_memory().is_some());
    let _operator_parents = operator_rows
        .then(|| crate::executor::batch::OperatorMemory::hold_rows(controls, &rows))
        .transpose()?;
    let _output_memory = if operator_rows {
        use crate::executor::retained_memory::{add, mul};
        let bytes = rows.iter().try_fold(
            mul(rows.len(), std::mem::size_of::<Vec<Value>>())?,
            |bytes, row| {
                add(
                    bytes,
                    mul(row.entries().len(), std::mem::size_of::<Value>())?,
                )
            },
        )?;
        Some(controls.reserve_query_memory(bytes)?)
    } else {
        None
    };
    let rows: Vec<Vec<Value>> = if operator_rows {
        materialize_operator_rows(rows, keep_internal_identity_as_id, controls)?
    } else {
        rows.into_iter()
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
            .collect()
    };

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

fn materialized_projection_schema(
    cassie: &Cassie,
    collection: &str,
) -> Option<crate::catalog::CollectionSchema> {
    let projection = cassie.catalog.get_materialized_projection(collection)?;
    let materialized = projection.materialized?;
    Some(crate::catalog::CollectionSchema::from_type_schema(
        collection.to_owned(),
        &materialized.output_schema,
    ))
}

fn materialize_operator_rows(
    rows: Vec<BatchRow>,
    keep_identity: bool,
    controls: &QueryExecutionControls,
) -> Result<Vec<Vec<Value>>, QueryError> {
    let allocation_error = |error: std::collections::TryReserveError| {
        QueryError::from(crate::app::CassieError::ResourceLimit(format!(
            "unable to materialize typed result: {error}"
        )))
    };
    let mut output = Vec::new();
    output
        .try_reserve_exact(rows.len())
        .map_err(allocation_error)?;
    for row in rows {
        super::check_timeout(controls)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(row.entries().len())
            .map_err(allocation_error)?;
        for (name, value) in row.into_entries() {
            if keep_identity || !crate::types::row_identity::is_row_identity_column(&name) {
                values.push(value);
            }
        }
        output.push(values);
    }
    super::check_timeout(controls)?;
    Ok(output)
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

            let field_key = crate::sql::ColumnIdentifierPath::stored_row_lookup_key(name);
            if fields.insert(field_key) {
                ordered.push(name.clone());
            }
        }
    }

    ordered
}
