//! Type lookup for visible source namespaces and lowered physical references.

use super::{
    ast, binder, field_type_map, infer_projected_field_types, source_field_type_map, FieldTypeMap,
    HashMap,
};

/// Maps each column a `FROM` source exposes to its declared type. A derived
/// table or CTE (declared in `ctes`) is typed by its own output columns, so a
/// rename such as `txt AS uid` keeps `txt`'s type.
pub(crate) fn source_field_type_map_with_ctes(
    source: &ast::QuerySource,
    ctes: &[ast::CommonTableExpression],
    catalog: &crate::catalog::Catalog,
) -> FieldTypeMap {
    source_field_type_map_with_parameters(source, ctes, catalog, &[])
}

pub(crate) fn source_field_type_map_with_parameters(
    source: &ast::QuerySource,
    ctes: &[ast::CommonTableExpression],
    catalog: &crate::catalog::Catalog,
    parameter_types: &[i32],
) -> FieldTypeMap {
    match source {
        ast::QuerySource::Values { rows } => values_field_type_map(rows, parameter_types),

        ast::QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => {
            let mut fields = FieldTypeMap::new();
            let qualifier = binder::alias_row_qualifier(alias);
            let visible_qualifier = super::ColumnIdentifierPath::parse(alias)
                .map_or_else(|_| alias.clone(), |path| path.namespace_key());
            if let Ok((shape, identity)) = binder::wildcard_output_shape_with_parameters(
                source,
                ctes,
                catalog,
                &HashMap::new(),
                parameter_types,
            ) {
                for (index, field) in shape.into_iter().enumerate() {
                    let visible = column_aliases.get(index).cloned().unwrap_or_else(|| {
                        super::ColumnIdentifierPath::stored_field_key(&field.name)
                    });
                    let physical = if identity && field.name == "id" {
                        "_id".into()
                    } else {
                        super::ColumnIdentifierPath::stored_field_key(&field.name)
                    };
                    fields.insert(visible.clone(), field.data_type.clone());
                    fields.insert(physical.clone(), field.data_type.clone());
                    fields.insert(
                        format!("{visible_qualifier}.{visible}"),
                        field.data_type.clone(),
                    );
                    fields.insert(format!("{qualifier}.{physical}"), field.data_type);
                }
            }
            fields
        }
        ast::QuerySource::Collection(collection) => collection_field_type_map(collection, catalog),
        ast::QuerySource::TableSample { source, .. } => {
            source_field_type_map_with_parameters(source, ctes, catalog, parameter_types)
        }
        ast::QuerySource::Join { left, right, .. } => {
            let mut fields =
                source_field_type_map_with_parameters(left, ctes, catalog, parameter_types);
            qualify_collection_fields(left, &mut fields);
            let mut right_fields =
                source_field_type_map_with_parameters(right, ctes, catalog, parameter_types);
            qualify_collection_fields(right, &mut right_fields);
            fields.extend(right_fields);
            fields
        }
        ast::QuerySource::Subquery { select, .. } => {
            if let Some(schema) = binder::derived_source_schema_with_parameters(
                source,
                ctes,
                catalog,
                &HashMap::new(),
                parameter_types,
            ) {
                return field_type_map(schema.fields.iter());
            }
            let mut fields = source_field_type_map(&select.source, catalog);
            for cte in &select.ctes {
                if let ast::CteQuery::Simple(statement) = &cte.query {
                    infer_projected_field_types(&statement.statement, catalog, &mut fields);
                }
            }
            fields
        }
        ast::QuerySource::Cte(_) => binder::derived_source_schema_with_parameters(
            source,
            ctes,
            catalog,
            &HashMap::new(),
            parameter_types,
        )
        .map_or_default(|schema| field_type_map(schema.fields.iter())),
        ast::QuerySource::TableFunction { .. } => binder::wildcard_output_shape_with_parameters(
            source,
            ctes,
            catalog,
            &HashMap::new(),
            parameter_types,
        )
        .map_or_default(|(fields, _)| {
            fields
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect()
        }),
        ast::QuerySource::SingleRow => FieldTypeMap::new(),
    }
}

/// Adds each column of a joined table under its qualified names too, so
/// `t.c` keeps `t`'s type when another join input has a column `c` (a
/// generated source names its output `column1`).
fn qualify_collection_fields(source: &ast::QuerySource, fields: &mut FieldTypeMap) {
    let ast::QuerySource::Collection(collection) = source else {
        return;
    };
    let qualified = fields
        .iter()
        .filter(|(key, _)| !key.contains('.'))
        .flat_map(|(key, data_type)| {
            crate::catalog::qualifier_variants(collection)
                .into_iter()
                .map(move |qualifier| (format!("{qualifier}.{key}"), data_type.clone()))
        })
        .collect::<Vec<_>>();
    fields.extend(qualified);
}

fn collection_field_type_map(collection: &str, catalog: &crate::catalog::Catalog) -> FieldTypeMap {
    if crate::catalog::virtual_views::borrowed_information_schema_tables(collection).is_some() {
        FieldTypeMap::new()
    } else if let Some(schema) = catalog.get_schema(collection) {
        field_type_map(schema.fields.iter())
    } else {
        pg_catalog_view_field_type_map(collection)
    }
}

/// Column types of a `pg_catalog` virtual view, so a parameter compared with
/// one of its columns (`WHERE oid = $1`) takes that column's type as it does
/// for a table column. Client type introspection depends on this.
fn pg_catalog_view_field_type_map(collection: &str) -> FieldTypeMap {
    let name = collection.trim().to_ascii_lowercase();
    let qualified = if name.contains('.') {
        name
    } else {
        format!("pg_catalog.{name}")
    };
    if !qualified.starts_with("pg_catalog.") {
        return FieldTypeMap::new();
    }
    crate::catalog::virtual_views::schema(&qualified).map_or_default(|fields| {
        fields
            .into_iter()
            .map(|(name, data_type)| {
                (
                    crate::sql::ColumnIdentifierPath::from_field_name(&name).lookup_key(),
                    data_type,
                )
            })
            .collect()
    })
}

fn values_field_type_map(rows: &[Vec<ast::Expr>], parameter_types: &[i32]) -> FieldTypeMap {
    binder::values::schema(rows, &HashMap::new(), parameter_types).map_or_default(|schema| {
        schema
            .fields
            .into_iter()
            .map(|field| (field.name, field.data_type))
            .collect()
    })
}
