use std::collections::HashMap;

use crate::catalog::{parse_name, FunctionMeta, ParsedName};

/// Builds the function names visible in one database and search path.
///
/// The returned map keeps canonical names for qualified references and adds
/// schema-qualified and unqualified aliases only for schemas in the search
/// path. In unscoped mode it preserves the legacy instance-wide catalog view.
#[must_use]
pub fn functions_for_scope(
    functions: &[FunctionMeta],
    database: &str,
    search_path: &[String],
    enforce_database_scope: bool,
) -> HashMap<String, FunctionMeta> {
    let mut visible = HashMap::new();
    let mut scoped = Vec::new();

    for function in functions {
        let Ok(ParsedName::DatabaseQualified {
            database: function_database,
            schema,
            name,
        }) = parse_name(&function.name)
        else {
            if !enforce_database_scope {
                visible.insert(function.name.to_ascii_lowercase(), function.clone());
            }
            continue;
        };

        if enforce_database_scope && !function_database.eq_ignore_ascii_case(database) {
            continue;
        }

        let canonical = function.name.to_ascii_lowercase();
        visible.insert(canonical, function.clone());
        visible
            .entry(format!("{schema}.{name}").to_ascii_lowercase())
            .or_insert_with(|| function.clone());
        scoped.push((schema, name, function.clone()));
    }

    if enforce_database_scope {
        for search_schema in search_path {
            for (schema, name, function) in &scoped {
                if schema.eq_ignore_ascii_case(search_schema) {
                    visible
                        .entry(name.to_ascii_lowercase())
                        .or_insert_with(|| function.clone());
                }
            }
        }
    } else {
        for (_, name, function) in scoped {
            visible.entry(name.to_ascii_lowercase()).or_insert(function);
        }
    }

    visible
}

/// Builds an instance-local compatibility map limited to one database.
///
/// Maintenance rebuilds do not have a live session search path. They still
/// must not execute functions owned by another database, so this exposes
/// aliases from schemas that contain functions in the requested database.
#[must_use]
pub fn functions_for_database(
    functions: &[FunctionMeta],
    database: &str,
) -> HashMap<String, FunctionMeta> {
    let mut schemas = Vec::new();
    for function in functions {
        if let Ok(ParsedName::DatabaseQualified {
            database: function_database,
            schema,
            ..
        }) = parse_name(&function.name)
        {
            if function_database.eq_ignore_ascii_case(database)
                && !schemas
                    .iter()
                    .any(|known: &String| known.eq_ignore_ascii_case(&schema))
            {
                schemas.push(schema);
            }
        }
    }
    if schemas.is_empty() {
        schemas.push(crate::catalog::DEFAULT_SCHEMA.to_string());
    }

    functions_for_scope(functions, database, &schemas, true)
}
