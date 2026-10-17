//! Output schema of a derived table or CTE used as a `FROM` source.
//!
//! A derived table or CTE is not a catalog object, so its column names and
//! types come from its body: `(SELECT txt AS uid FROM t) d` exposes a TEXT
//! column `uid`, whatever type a same-named column of `t` (or a table named
//! `d`) has.

use super::inference::{infer_cte_schema, infer_source_schema_with_parameters};
use super::{Catalog, CommonTableExpression, HashMap, QuerySource, Schema};

/// Returns the output schema of `source` when it is a derived table or a CTE
/// declared in `ctes`, or `None` for any other source or when the body's
/// schema cannot be inferred.
#[must_use]
pub(crate) fn derived_source_schema(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Option<crate::catalog::CollectionSchema> {
    derived_source_schema_with_parameters(source, ctes, catalog, user_functions, &[])
}

pub(crate) fn derived_source_schema_with_parameters(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    parameter_types: &[i32],
) -> Option<crate::catalog::CollectionSchema> {
    let (name, schema) = match source {
        QuerySource::Values { rows } => {
            let schema = super::values::schema(rows, user_functions, parameter_types).ok()?;
            return Some(crate::catalog::CollectionSchema::from_type_schema(
                "values", &schema,
            ));
        }
        QuerySource::Aliased {
            alias,
            source: inner,
            ..
        } if matches!(
            inner.as_ref(),
            QuerySource::Values { .. } | QuerySource::Subquery { .. }
        ) =>
        {
            let schema = infer_source_schema_with_parameters(
                source,
                catalog,
                &cte_schemas_with_parameters(ctes, catalog, user_functions, parameter_types),
                user_functions,
                true,
                parameter_types,
            )
            .ok()?;
            (alias, schema)
        }
        QuerySource::Cte(name) => {
            let schemas =
                cte_schemas_with_parameters(ctes, catalog, user_functions, parameter_types);
            (name, schemas.get(&name.to_ascii_lowercase())?.clone())
        }
        QuerySource::Subquery { alias, select, .. } => {
            let schemas =
                cte_schemas_with_parameters(ctes, catalog, user_functions, parameter_types);
            let schema = super::inference::infer_select_schema_with_parameters(
                select,
                catalog,
                &schemas,
                user_functions,
                parameter_types,
            )
            .ok()?;
            (alias, schema)
        }
        _ => return None,
    };
    Some(crate::catalog::CollectionSchema {
        collection: name.clone(),
        fields: schema
            .fields
            .into_iter()
            .map(|field| crate::catalog::FieldMeta {
                name: field.name,
                data_type: field.data_type,
                is_indexed: false,
                boost: None,
            })
            .collect(),
    })
}

/// Resolves the existing join source's fields for ordinary result metadata.
/// This schema is not a physical row identity or result-filtering decision.
#[must_use]
pub(crate) fn joined_source_schema(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> Option<crate::catalog::CollectionSchema> {
    if !matches!(source, QuerySource::Join { .. }) {
        return None;
    }
    let schemas = cte_schemas(ctes, catalog, user_functions);
    let schema =
        infer_source_schema_with_parameters(source, catalog, &schemas, user_functions, true, &[])
            .ok()?;
    Some(crate::catalog::CollectionSchema {
        // This is the existing logical::source_name label, not a catalog object.
        collection: "join".to_string(),
        fields: schema
            .fields
            .into_iter()
            .map(|field| crate::catalog::FieldMeta {
                name: field.name,
                data_type: field.data_type,
                is_indexed: false,
                boost: None,
            })
            .collect(),
    })
}

fn cte_schemas(
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
) -> HashMap<String, Schema> {
    let mut schemas = HashMap::new();
    for cte in ctes {
        let Ok(schema) = infer_cte_schema(cte, catalog, &schemas, user_functions) else {
            break;
        };
        schemas.insert(cte.name.to_ascii_lowercase(), schema);
    }
    schemas
}

fn cte_schemas_with_parameters(
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    functions: &HashMap<String, crate::catalog::FunctionMeta>,
    params: &[i32],
) -> HashMap<String, Schema> {
    let mut schemas = HashMap::new();
    for cte in ctes {
        let Ok(schema) = super::inference::infer_cte_schema_with_parameters(
            cte, catalog, &schemas, functions, params,
        ) else {
            break;
        };
        schemas.insert(cte.name.to_ascii_lowercase(), schema);
    }
    schemas
}
