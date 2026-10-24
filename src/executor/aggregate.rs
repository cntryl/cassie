use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use crate::catalog::{CollectionSchema, FunctionMeta};
use crate::executor::ColumnMeta;
use crate::sql::ast::{
    CommonTableExpression, Expr, FunctionCall, QuerySource, SelectItem, WindowFunctionCall,
};
use crate::types::row_identity::{is_row_identity_column, LEGACY_ID_COLUMN};
use crate::types::{DataType, FieldSchema, Schema};

#[must_use]
pub fn columns_from_projection<S: BuildHasher>(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta, S>,
) -> Vec<ColumnMeta> {
    columns_from_projection_with_parameter_oids(projection, collection_schema, user_functions, &[])
}

#[must_use]
pub fn columns_from_projection_with_parameter_oids<S: BuildHasher>(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta, S>,
    parameter_type_oids: &[i32],
) -> Vec<ColumnMeta> {
    columns_from_projection_with_wildcard(
        projection,
        collection_schema,
        None,
        user_functions,
        parameter_type_oids,
    )
}

/// Resolves the row shape `SELECT *` expands to for `logical`'s source, or
/// `None` when the projection has no wildcard or the source cannot be
/// inferred (callers then keep the collection-schema expansion).
#[must_use]
pub(crate) fn wildcard_fields_for_plan(
    catalog: &crate::catalog::Catalog,
    logical: &crate::planner::logical::LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Option<Vec<FieldSchema>> {
    wildcard_shape_for_plan(catalog, logical, user_functions).map(|(fields, _)| fields)
}

/// Resolves displayed wildcard fields and the projected internal identity.
#[must_use]
pub(crate) fn wildcard_shape_for_plan(
    catalog: &crate::catalog::Catalog,
    logical: &crate::planner::logical::LogicalPlan,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Option<(Vec<FieldSchema>, bool)> {
    if !logical
        .projection
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return None;
    }
    // A base table keeps the collection-schema expansion, which honours the
    // schema snapshot a saved plan was compiled against.
    if let crate::sql::ast::QuerySource::Collection(name) = &logical.source {
        let names_cte = logical
            .ctes
            .iter()
            .any(|cte| cte.name.eq_ignore_ascii_case(name));
        if !names_cte
            && catalog.get_view(name).is_none()
            && catalog.get_materialized_projection(name).is_none()
        {
            return None;
        }
    }
    crate::sql::binder::wildcard_output_fields_and_identity(
        &logical.source,
        &logical.ctes,
        catalog,
        user_functions,
    )
    .ok()
}

/// Applies the existing DML wildcard identity presentation to result columns.
/// The first legacy `id` name is relabelled only for a wildcard projection
/// against a target that declares no `id` field.
pub(crate) fn normalize_dml_returning_identity_columns(
    columns: &mut [ColumnMeta],
    returning: &[SelectItem],
    schema_has_id: bool,
) {
    if schema_has_id
        || !returning
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return;
    }
    if let Some(column) = columns
        .iter_mut()
        .find(|column| crate::types::row_identity::is_legacy_id_column(&column.name))
    {
        column.name = crate::types::row_identity::ROW_IDENTITY_COLUMN.to_string();
    }
}

/// Builds result columns, expanding `*` to `wildcard_fields` when the caller
/// resolved the source's row shape (see `sql::binder::wildcard_output_fields`).
#[must_use]
pub(crate) fn columns_from_projection_with_wildcard<S: BuildHasher>(
    projection: &[SelectItem],
    collection_schema: Option<&CollectionSchema>,
    wildcard_fields: Option<&[FieldSchema]>,
    user_functions: &HashMap<String, FunctionMeta, S>,
    parameter_type_oids: &[i32],
) -> Vec<ColumnMeta> {
    if projection.is_empty() {
        return vec![ColumnMeta::from_data_type("*", &DataType::Text)];
    }

    let source_schema = projection_source_schema(collection_schema);
    let user_functions = user_functions
        .iter()
        .map(|(name, metadata)| (name.clone(), metadata.clone()))
        .collect::<HashMap<_, _>>();

    let mut columns = projection
        .iter()
        .flat_map(|item| match item {
            SelectItem::Wildcard => wildcard_columns(collection_schema, wildcard_fields),
            SelectItem::Column { name, alias } => {
                let data_type = column_data_type(name, collection_schema);
                let output_name = alias.clone().unwrap_or_else(|| {
                    crate::sql::ColumnIdentifierPath::parse(name)
                        .map_or_else(|_| name.clone(), |column| column.implicit_output_label())
                });
                vec![ColumnMeta::from_data_type(output_name, &data_type)]
            }
            SelectItem::Function { function, alias } => {
                let data_type = crate::sql::binder::infer_function_return_type(
                    function,
                    &source_schema,
                    &user_functions,
                    parameter_type_oids,
                )
                .unwrap_or(DataType::Text);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| function.name.clone()),
                    &data_type,
                )]
            }
            SelectItem::Expr { expr, alias } => {
                let data_type = crate::sql::binder::infer_projection_expr_type(
                    expr,
                    &source_schema,
                    &user_functions,
                    parameter_type_oids,
                )
                .unwrap_or(DataType::Text);
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| "expr".to_string()),
                    &data_type,
                )]
            }
            SelectItem::WindowFunction { function, alias } => {
                let data_type = window_result_type(
                    function,
                    &source_schema,
                    &user_functions,
                    parameter_type_oids,
                );
                vec![ColumnMeta::from_data_type(
                    alias.clone().unwrap_or_else(|| function.name.clone()),
                    &data_type,
                )]
            }
        })
        .collect::<Vec<_>>();
    apply_percentile_array_oids(projection, &mut columns);
    apply_void_function_oids(projection, &mut columns);
    columns
}

/// Applies PostgreSQL's output-only pseudo-type identity to direct `void`
/// function projections without adding `void` to Cassie's persisted type set.
pub(crate) fn apply_void_function_oids(projection: &[SelectItem], columns: &mut [ColumnMeta]) {
    if projection.len() != columns.len()
        || projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return;
    }
    for (item, column) in projection.iter().zip(columns) {
        let is_void = match item {
            SelectItem::Function { function, .. } => is_void_result_function(function),
            SelectItem::Expr { expr, .. } => is_void_result_expression(expr),
            _ => false,
        };
        if is_void {
            column.type_oid = 2278;
            column.typlen = 4;
            column.atttypmod = -1;
        }
    }
}

/// Applies PostgreSQL's internal one-byte `char` identity to the
/// `pg_type.typtype` result column without changing Cassie's SQL text type.
pub(crate) fn apply_pg_type_kind_oid(
    source: &crate::sql::ast::QuerySource,
    projection: &[SelectItem],
    columns: &mut [ColumnMeta],
) {
    if !query_source_contains_pg_type(source) {
        return;
    }

    if projection
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        for column in columns {
            if column.name.eq_ignore_ascii_case("typtype") {
                apply_internal_char_identity(column);
            }
        }
        return;
    }

    for (item, column) in projection.iter().zip(columns) {
        if is_pg_type_kind_column(item) {
            apply_internal_char_identity(column);
        }
    }
}

fn query_source_contains_pg_type(source: &crate::sql::ast::QuerySource) -> bool {
    use crate::sql::ast::QuerySource;
    match source {
        QuerySource::Collection(name) => {
            name.to_string().eq_ignore_ascii_case("pg_catalog.pg_type")
        }
        QuerySource::Aliased { source, .. } => query_source_contains_pg_type(source),
        QuerySource::Join { left, right, .. } => {
            query_source_contains_pg_type(left) || query_source_contains_pg_type(right)
        }
        QuerySource::Subquery { select, .. } => query_source_contains_pg_type(&select.source),
        QuerySource::Values { .. }
        | QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::SingleRow => false,
    }
}

fn is_pg_type_kind_column(item: &SelectItem) -> bool {
    match item {
        SelectItem::Column { name, .. } => {
            crate::sql::ColumnIdentifierPath::parse(name).is_ok_and(|column| {
                column
                    .implicit_output_label()
                    .eq_ignore_ascii_case("typtype")
            })
        }
        _ => false,
    }
}

fn apply_internal_char_identity(column: &mut ColumnMeta) {
    "char".clone_into(&mut column.data_type);
    column.type_oid = 18;
    column.typlen = 1;
    column.atttypmod = -1;
}

fn is_void_function(function: &FunctionCall) -> bool {
    crate::sql::functions::function(&function.name).is_some_and(|metadata| {
        matches!(
            metadata.id,
            crate::sql::functions::FunctionId::PgSleep
                | crate::sql::functions::FunctionId::PgSleepFor
                | crate::sql::functions::FunctionId::PgSleepUntil
        )
    })
}

fn is_void_result_function(function: &FunctionCall) -> bool {
    if is_void_function(function) {
        return true;
    }
    let is_coalesce = crate::sql::functions::function(&function.name)
        .is_some_and(|metadata| metadata.id == crate::sql::functions::FunctionId::Coalesce);
    is_coalesce
        && function.args.iter().any(is_void_result_expression)
        && function.args.iter().all(is_void_or_null_result_expression)
}

fn is_void_result_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Function(function) => is_void_result_function(function),
        Expr::Case {
            branches,
            else_expr,
            ..
        } => {
            let has_void_result = branches
                .iter()
                .any(|(_, result)| is_void_result_expression(result))
                || else_expr.as_deref().is_some_and(is_void_result_expression);
            has_void_result
                && branches
                    .iter()
                    .all(|(_, result)| is_void_or_null_result_expression(result))
                && else_expr
                    .as_deref()
                    .is_none_or(is_void_or_null_result_expression)
        }
        _ => false,
    }
}

fn is_void_or_null_result_expression(expression: &Expr) -> bool {
    matches!(expression, Expr::Null) || is_void_result_expression(expression)
}

/// Applies PostgreSQL's built-in array OID to percentile array results whose
/// evaluator carrier does not retain the SQL array type identity.
pub(crate) fn apply_percentile_array_oids(projection: &[SelectItem], columns: &mut [ColumnMeta]) {
    let _ =
        apply_percentile_array_oids_for_query(&QuerySource::SingleRow, &[], projection, columns);
}

pub(crate) fn apply_percentile_array_oids_for_query(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    projection: &[SelectItem],
    columns: &mut [ColumnMeta],
) -> Vec<usize> {
    if projection.len() != columns.len()
        || projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return Vec::new();
    }
    let mut array_text_columns = Vec::new();
    for (index, (item, column)) in projection.iter().zip(columns).enumerate() {
        let Ok(data_type) = DataType::parse_sql(&column.data_type) else {
            continue;
        };
        let output_oid = match item {
            SelectItem::Function { function, .. } => {
                percentile_array_output_oid(function, &data_type)
            }
            SelectItem::Expr {
                expr: Expr::Function(function),
                ..
            } => percentile_array_output_oid(function, &data_type),
            SelectItem::Column { name, .. } => {
                crate::sql::functions::percentile_function_for_column(source, ctes, name).and_then(
                    |function_id| percentile_array_output_oid_for_id(function_id, &data_type),
                )
            }
            SelectItem::Expr {
                expr: Expr::Column(name),
                ..
            } => crate::sql::functions::percentile_function_for_column(source, ctes, name)
                .and_then(|function_id| {
                    percentile_array_output_oid_for_id(function_id, &data_type)
                }),
            _ => None,
        };
        if let Some(oid) = output_oid {
            column.type_oid = oid;
            array_text_columns.push(index);
        }
    }
    array_text_columns
}

/// Applies PostgreSQL's ephemeral RECORD OIDs to direct recursive SEARCH and
/// CYCLE outputs while retaining Cassie's text carrier and storage identity.
pub(crate) fn apply_recursive_cte_record_oids_for_query(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    projection: &[SelectItem],
    columns: &mut [ColumnMeta],
) {
    let mut referenced = HashSet::new();
    collect_cte_sources(source, &mut referenced);
    let mut identities = HashMap::<String, Option<i64>>::new();
    for cte in ctes {
        if !referenced.contains(&cte.name.to_ascii_lowercase()) {
            continue;
        }
        if let Some(search) = &cte.search {
            let oid = match search.order {
                crate::sql::ast::CteSearchOrder::DepthFirst => 2287,
                crate::sql::ast::CteSearchOrder::BreadthFirst => 2249,
            };
            insert_record_identity(&mut identities, &search.sequence_column, oid);
        }
        if let Some(cycle) = &cte.cycle {
            let empty_schema = Schema { fields: Vec::new() };
            let no_functions = HashMap::new();
            let mark_type = crate::sql::binder::infer_expr_type(
                &cycle.mark_value,
                &empty_schema,
                &no_functions,
                &[],
            );
            let default_type = crate::sql::binder::infer_expr_type(
                &cycle.default_value,
                &empty_schema,
                &no_functions,
                &[],
            );
            let mark_type = match (mark_type, default_type) {
                (Some(DataType::Null), Some(other)) | (Some(other), Some(DataType::Null)) => {
                    Some(other)
                }
                (Some(mark), Some(default)) => crate::sql::binder::common_case_type(mark, default),
                _ => None,
            };
            let mark_oid = mark_type
                .filter(|data_type| *data_type != DataType::Null)
                .unwrap_or(DataType::Text)
                .type_oid();
            insert_record_identity(&mut identities, &cycle.mark_column, mark_oid);
            insert_record_identity(&mut identities, &cycle.path_column, 2287);
        }
    }
    if identities.is_empty() {
        return;
    }
    if projection
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        for column in columns {
            let Some(Some(oid)) = identities.get(&column.name) else {
                continue;
            };
            column.type_oid = *oid;
        }
        return;
    }
    if projection.len() != columns.len() {
        return;
    }
    for (item, column) in projection.iter().zip(columns) {
        let Some(name) = direct_column_name(item) else {
            continue;
        };
        let Some(Some(oid)) = identities.get(&name) else {
            continue;
        };
        column.type_oid = *oid;
    }
}

fn insert_record_identity(identities: &mut HashMap<String, Option<i64>>, name: &str, oid: i64) {
    match identities.entry(name.to_owned()) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(oid));
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get() != &Some(oid) {
                entry.insert(None);
            }
        }
    }
}

fn direct_column_name(item: &SelectItem) -> Option<String> {
    let (SelectItem::Column { name, .. }
    | SelectItem::Expr {
        expr: Expr::Column(name),
        ..
    }) = item
    else {
        return None;
    };
    crate::sql::ColumnIdentifierPath::parse(name)
        .ok()
        .map(|column| column.final_name().to_owned())
}

fn collect_cte_sources(source: &QuerySource, names: &mut HashSet<String>) {
    match source {
        QuerySource::Cte(name) => {
            names.insert(name.to_ascii_lowercase());
        }
        QuerySource::Aliased { source, .. } => collect_cte_sources(source, names),
        QuerySource::Join { left, right, .. } => {
            collect_cte_sources(left, names);
            collect_cte_sources(right, names);
        }
        QuerySource::Subquery { select, .. } => collect_cte_sources(&select.source, names),
        QuerySource::Values { .. }
        | QuerySource::Collection(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::SingleRow => {}
    }
}

pub(crate) fn percentile_array_output_oid(
    function: &FunctionCall,
    data_type: &DataType,
) -> Option<i64> {
    let metadata = crate::sql::functions::function(&function.name)?;
    percentile_array_output_oid_for_id(metadata.id, data_type)
}

fn percentile_array_output_oid_for_id(
    function_id: crate::sql::functions::FunctionId,
    data_type: &DataType,
) -> Option<i64> {
    if !matches!(
        function_id,
        crate::sql::functions::FunctionId::PercentileCont
            | crate::sql::functions::FunctionId::PercentileDisc
    ) {
        return None;
    }
    let DataType::Array(element) = data_type else {
        return None;
    };
    let element_oid = u32::try_from(element.type_oid()).ok()?;
    crate::catalog::builtin_identity::builtin_array_oid(element_oid).map(i64::from)
}

fn wildcard_columns(
    collection_schema: Option<&CollectionSchema>,
    wildcard_fields: Option<&[FieldSchema]>,
) -> Vec<ColumnMeta> {
    if let Some(fields) = wildcard_fields {
        fields
            .iter()
            .map(|field| ColumnMeta::from_data_type(field.name.clone(), &field.data_type))
            .collect()
    } else if let Some(collection_schema) = collection_schema {
        if crate::catalog::virtual_views::schema(&collection_schema.collection).is_some() {
            collection_schema
                .fields
                .iter()
                .map(|field| ColumnMeta::from_data_type(field.name.clone(), &field.data_type))
                .collect()
        } else {
            let mut columns = Vec::with_capacity(collection_schema.fields.len() + 1);
            let mut seen = HashSet::new();
            if !collection_schema.declares_id() {
                seen.insert(LEGACY_ID_COLUMN.to_string());
                columns.push(ColumnMeta::from_data_type(
                    LEGACY_ID_COLUMN,
                    &DataType::Text,
                ));
            }
            for field in &collection_schema.fields {
                if seen.insert(crate::sql::ColumnIdentifierPath::stored_field_key(
                    &field.name,
                )) {
                    columns.push(ColumnMeta::from_data_type(
                        field.name.clone(),
                        &field.data_type,
                    ));
                }
            }
            columns.into_iter().collect()
        }
    } else {
        vec![ColumnMeta::from_data_type("*", &DataType::Text)]
    }
}

fn window_result_type(
    function: &WindowFunctionCall,
    source_schema: &Schema,
    user_functions: &HashMap<String, FunctionMeta>,
    parameter_type_oids: &[i32],
) -> DataType {
    crate::sql::binder::infer_window_return_type(
        function,
        source_schema,
        user_functions,
        parameter_type_oids,
    )
}

fn projection_source_schema(collection_schema: Option<&CollectionSchema>) -> Schema {
    let Some(collection_schema) = collection_schema else {
        return Schema { fields: Vec::new() };
    };

    let mut fields = Vec::with_capacity(collection_schema.fields.len() + 1);
    if !collection_schema.declares_id() {
        fields.push(FieldSchema {
            name: LEGACY_ID_COLUMN.to_string(),
            data_type: DataType::Text,
            nullable: true,
        });
    }
    fields.extend(collection_schema.fields.iter().map(|field| FieldSchema {
        name: field.name.clone(),
        data_type: field.data_type.clone(),
        nullable: true,
    }));
    Schema { fields }
}

fn column_data_type(name: &str, schema: Option<&CollectionSchema>) -> DataType {
    if is_row_identity_column(name) {
        return DataType::Text;
    }

    let Some(schema) = schema else {
        return DataType::Text;
    };

    // "id" is a normal column when the schema declares one (its own type
    // applies), otherwise a bare "id" reference was already rewritten to
    // "_id" before reaching here (see
    // `planner::logical::rewrite_reserved_id_references`), so falling
    // through to "not found in schema" -> Text is unreachable for it, not a
    // silent wrong guess.
    schema
        .fields
        .iter()
        .find(|field| {
            crate::sql::ColumnIdentifierPath::parse(name).is_ok_and(|reference| {
                reference.is_qualified()
                    && crate::sql::ColumnIdentifierPath::parse(&field.name).is_ok_and(|candidate| {
                        candidate.is_qualified() && candidate.lookup_key() == reference.lookup_key()
                    })
            })
        })
        .or_else(|| {
            schema.fields.iter().find(|field| {
                crate::sql::ColumnIdentifierPath::stored_field_key(&field.name)
                    == crate::sql::ColumnIdentifierPath::reference_field_key(name)
            })
        })
        .map_or(DataType::Text, |field| field.data_type.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::columns_from_projection;
    use crate::sql::ast::{FunctionCall, SelectItem};
    use crate::types::DataType;

    #[test]
    fn should_report_count_results_as_bigint() {
        // Arrange
        let projection = vec![SelectItem::Function {
            function: FunctionCall {
                name: "count".to_string(),
                args: vec![],
                ..Default::default()
            },
            alias: None,
        }];

        // Act
        let columns = columns_from_projection(&projection, None, &HashMap::new());

        // Assert
        assert_eq!(columns[0].type_oid, DataType::BigInt.type_oid());
    }
}
