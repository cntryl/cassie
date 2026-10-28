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
        ast::QuerySource::Values { rows } => {
            binder::values::schema(rows, &HashMap::new(), parameter_types)
                .map(|schema| {
                    schema
                        .fields
                        .into_iter()
                        .map(|field| (field.name, field.data_type))
                        .collect()
                })
                .unwrap_or_default()
        }

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
                    fields.insert(
                        format!("{visible_qualifier}.{visible}"),
                        field.data_type.clone(),
                    );
                    fields.insert(format!("{qualifier}.{physical}"), field.data_type);
                }
            }
            fields
        }
        ast::QuerySource::Collection(collection) => catalog
            .get_schema(collection)
            .map(|schema| field_type_map(schema.fields.iter()))
            .unwrap_or_default(),
        ast::QuerySource::TableSample { source, .. } => {
            source_field_type_map_with_parameters(source, ctes, catalog, parameter_types)
        }
        ast::QuerySource::Join { left, right, .. } => {
            let mut fields =
                source_field_type_map_with_parameters(left, ctes, catalog, parameter_types);
            fields.extend(source_field_type_map_with_parameters(
                right,
                ctes,
                catalog,
                parameter_types,
            ));
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
        .map(|schema| field_type_map(schema.fields.iter()))
        .unwrap_or_default(),
        ast::QuerySource::TableFunction { .. } | ast::QuerySource::SingleRow => FieldTypeMap::new(),
    }
}
