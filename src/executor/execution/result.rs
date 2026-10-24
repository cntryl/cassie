use super::{
    aggregate, BatchRow, Cassie, CmpOrdering, FunctionMeta, HashMap, HashSet, PhysicalPlan,
    QueryError, QueryExecutionControls, QueryResult, Value,
};
use crate::executor::batch::RowAccess;
use crate::executor::semantic::{compare_values, SemanticKey};

fn apply_query_output_oids(
    plan: &PhysicalPlan,
    columns: &mut [crate::executor::ColumnMeta],
) -> Vec<usize> {
    let array_text_columns = aggregate::apply_percentile_array_oids_for_query(
        &plan.logical.source,
        &plan.logical.ctes,
        &plan.logical.projection,
        columns,
    );
    aggregate::apply_recursive_cte_record_oids_for_query(
        &plan.logical.source,
        &plan.logical.ctes,
        &plan.logical.projection,
        columns,
    );
    aggregate::apply_void_function_oids(&plan.logical.projection, columns);
    aggregate::apply_pg_type_kind_oid(&plan.logical.source, &plan.logical.projection, columns);
    array_text_columns
}

pub(super) fn build_select_result(
    cassie: &Cassie,
    session: Option<&crate::app::CassieSession>,
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
    let mut columns = aggregate::columns_from_projection_with_wildcard(
        &plan.logical.projection,
        collection_schema.as_ref(),
        wildcard_fields,
        user_functions,
        &[],
    );
    let array_text_columns = apply_query_output_oids(plan, &mut columns);
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
    // Keep typed parents alive while the managed result admits and copies output values.
    let operator_rows = rows.iter().any(|row| row.operator_memory().is_some());
    let _operator_parents = operator_rows
        .then(|| crate::executor::batch::OperatorMemory::hold_rows(controls, &rows))
        .transpose()?;
    if rows.len() > controls.max_result_rows {
        return Err(QueryError::General(format!(
            "query result row limit exceeded: {} > {}",
            rows.len(),
            controls.max_result_rows
        )));
    }

    let mut result = QueryResult::try_builder(controls)?;
    for column in &columns {
        result.try_push_column(column)?;
    }
    let object_labels = ObjectLabels::for_columns(cassie, session, &columns);
    let mut object_oids = object_labels
        .as_ref()
        .map(|labels| labels.oid_columns(rows.len()))
        .unwrap_or_default();
    for row in &rows {
        let values = row.entries().iter().filter_map(|(name, value)| {
            (!name.starts_with('\0')
                && (keep_internal_identity_as_id
                    || !crate::types::row_identity::is_row_identity_column(name)))
            .then_some(value)
        });
        match &object_labels {
            Some(labels) => {
                let labelled = labels.apply(values, &mut object_oids);
                result.try_push_values(labelled.iter())?;
            }
            None => result.try_push_values(values)?,
        }
    }
    for (column, oids) in object_oids {
        result.try_set_object_oids(column, oids)?;
    }
    result.try_set_postgres_array_text_columns(&array_text_columns)?;
    result.try_set_command("SELECT")?;
    let output = result.finish()?;
    // Retire the source rows only after the complete managed output is admitted.
    drop(rows);
    Ok(output)
}

/// `reg*` output: a result column of an object identifier alias type prints
/// the object's name, resolved against the session's search path.
struct ObjectLabels {
    catalog: crate::catalog::Catalog,
    context: crate::sql::binder::BindingContext,
    kinds: Vec<Option<crate::types::ObjectIdKind>>,
}

impl ObjectLabels {
    fn for_columns(
        cassie: &Cassie,
        session: Option<&crate::app::CassieSession>,
        columns: &[crate::executor::ColumnMeta],
    ) -> Option<Self> {
        let kinds = columns
            .iter()
            .map(|column| {
                crate::types::ObjectIdKind::ALL
                    .into_iter()
                    .find(|kind| kind.is_alias() && i64::from(kind.type_oid()) == column.type_oid)
            })
            .collect::<Vec<_>>();
        kinds.iter().any(Option::is_some).then(|| Self {
            catalog: cassie.catalog.clone(),
            context: cassie.binding_context_for_session(session),
            kinds,
        })
    }

    fn oid_columns(&self, rows: usize) -> Vec<(usize, Vec<Option<u32>>)> {
        self.kinds
            .iter()
            .enumerate()
            .filter(|(_, kind)| kind.is_some())
            .map(|(column, _)| (column, Vec::with_capacity(rows)))
            .collect()
    }

    /// Renders one row, recording each rendered value's OID for binary output.
    fn apply<'v>(
        &self,
        values: impl Iterator<Item = &'v Value>,
        oids: &mut [(usize, Vec<Option<u32>>)],
    ) -> Vec<Value> {
        let scope = crate::catalog::object_identity::ObjectScope::new(&self.catalog, &self.context);
        values
            .enumerate()
            .map(|(index, value)| {
                let Some(kind) = self.kinds.get(index).copied().flatten() else {
                    return value.clone();
                };
                let oid = match value {
                    Value::Int64(oid) => u32::try_from(*oid).ok(),
                    _ => None,
                };
                if let Some((_, column)) = oids.iter_mut().find(|(column, _)| *column == index) {
                    column.push(oid);
                }
                oid.map_or_else(
                    || value.clone(),
                    |oid| Value::String(scope.label(kind, oid)),
                )
            })
            .collect()
    }
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

pub(super) fn compare_query_values(left: &Value, right: &Value) -> CmpOrdering {
    compare_values(left, right)
}

pub(super) fn row_signature(row: &impl RowAccess) -> SemanticKey {
    SemanticKey::from_semantic_values(
        row.entries()
            .iter()
            .enumerate()
            .map(|(index, (_, value))| {
                crate::executor::semantic::typed_value(value, row.entry_type(index))
            })
            .collect(),
    )
}

pub(super) fn deduce_text_fields<R: RowAccess>(rows: &[R]) -> Vec<String> {
    let mut fields = HashSet::<String>::new();
    let mut ordered = Vec::new();

    for row in rows {
        for (name, value) in row.entries() {
            if name.starts_with('\0') || !matches!(value, Value::String(_) | Value::Json(_)) {
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
