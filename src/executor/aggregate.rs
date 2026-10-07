use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

use crate::catalog::{CollectionSchema, FunctionMeta};
use crate::executor::ColumnMeta;
use crate::sql::ast::{SelectItem, WindowFunctionCall};
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

    projection
        .iter()
        .flat_map(|item| match item {
            SelectItem::Wildcard => wildcard_columns(collection_schema, wildcard_fields),
            SelectItem::Column { name, alias } => {
                let data_type = column_data_type(name, collection_schema);
                let output_name = alias.clone().unwrap_or_else(|| {
                    crate::sql::ColumnIdentifierPath::parse(name)
                        .map_or_else(|_| name.clone(), |column| column.display_name())
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
                let data_type = crate::sql::binder::infer_expr_type(
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
        .collect()
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
    match function.name.to_ascii_lowercase().as_str() {
        "lag" | "lead" | "first_value" | "last_value" => function
            .args
            .first()
            .and_then(|arg| {
                crate::sql::binder::infer_expr_type(
                    arg,
                    source_schema,
                    user_functions,
                    parameter_type_oids,
                )
            })
            .unwrap_or(DataType::Text),
        _ => DataType::BigInt,
    }
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
            },
            alias: None,
        }];

        // Act
        let columns = columns_from_projection(&projection, None, &HashMap::new());

        // Assert
        assert_eq!(columns[0].type_oid, DataType::BigInt.type_oid());
    }
}
