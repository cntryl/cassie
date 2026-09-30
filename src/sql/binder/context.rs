use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, parse_name, Catalog,
    ParsedName, DEFAULT_SCHEMA,
};

#[derive(Debug, Clone)]
pub struct BindingContext {
    pub database: String,
    pub search_path: Vec<String>,
    pub enforce_database_scope: bool,
}

impl Default for BindingContext {
    fn default() -> Self {
        Self::unscoped("postgres", vec![DEFAULT_SCHEMA.to_string()])
    }
}

impl BindingContext {
    #[must_use]
    pub fn new(database: impl Into<String>, search_path: Vec<String>) -> Self {
        Self::scoped(database, search_path)
    }

    #[must_use]
    pub fn scoped(database: impl Into<String>, search_path: Vec<String>) -> Self {
        Self {
            database: database.into(),
            search_path: normalize_search_path(search_path),
            enforce_database_scope: true,
        }
    }

    #[must_use]
    pub fn unscoped(database: impl Into<String>, search_path: Vec<String>) -> Self {
        Self {
            database: database.into(),
            search_path: normalize_search_path(search_path),
            enforce_database_scope: false,
        }
    }

    #[must_use]
    pub fn current_schema(&self) -> &str {
        self.search_path
            .first()
            .map_or(DEFAULT_SCHEMA, String::as_str)
    }

    /// Schemas searched for an unqualified name. Like PostgreSQL, `pg_catalog`
    /// is searched first unless `search_path` places it explicitly.
    #[must_use]
    pub fn relation_search_order(&self) -> Vec<&str> {
        let mut order = Vec::with_capacity(self.search_path.len() + 1);
        if !self
            .search_path
            .iter()
            .any(|schema| schema.eq_ignore_ascii_case("pg_catalog"))
        {
            order.push("pg_catalog");
        }
        order.extend(self.search_path.iter().map(String::as_str));
        order
    }

    #[must_use]
    pub const fn scopes_database_objects(&self) -> bool {
        self.enforce_database_scope
    }
}

#[must_use]
pub fn normalize_search_path(path: Vec<String>) -> Vec<String> {
    let mut normalized = path
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        normalized.push(DEFAULT_SCHEMA.to_string());
    }
    normalized
}

/// # Errors
///
/// Returns an error when the relation reference is malformed or cross-database.
pub fn normalize_relation_name(raw: &str, context: &BindingContext) -> Result<String, CassieError> {
    if !context.scopes_database_objects() {
        return match parse_name(raw).map_err(CassieError::Planner)? {
            ParsedName::Unqualified(name) => Ok(name),
            ParsedName::SchemaQualified { schema, name } => Ok(format!("{schema}.{name}")),
            ParsedName::DatabaseQualified { .. } => Err(CassieError::Unsupported(
                "cross-database relation references are not supported".to_string(),
            )),
        };
    }

    match parse_name(raw).map_err(CassieError::Planner)? {
        ParsedName::Unqualified(name) => Ok(canonical_relation_name(
            &context.database,
            context.current_schema(),
            &name,
        )),
        ParsedName::SchemaQualified { schema, name } => {
            if is_system_schema(&schema) {
                return Ok(format!("{schema}.{name}"));
            }
            Ok(canonical_relation_name(&context.database, &schema, &name))
        }
        ParsedName::DatabaseQualified { .. } => Err(CassieError::Unsupported(
            "cross-database relation references are not supported".to_string(),
        )),
    }
}

/// Normalizes the name of a relation or routine being created or renamed.
///
/// # Errors
///
/// Returns `InsufficientPrivilege` when the target lands in a system schema,
/// explicitly or through `search_path`, because system schemas are not scoped
/// to a database; `CatalogObjectNotFound` (SQLSTATE `3F000`) when the target
/// schema does not exist; otherwise as [`normalize_relation_name`].
pub fn normalize_new_relation_name(
    raw: &str,
    context: &BindingContext,
    catalog: &Catalog,
) -> Result<String, CassieError> {
    let target_schema = match parse_name(raw).map_err(CassieError::Planner)? {
        ParsedName::Unqualified(_) => Some(context.current_schema().to_string()),
        ParsedName::SchemaQualified { schema, .. } => Some(schema),
        ParsedName::DatabaseQualified { .. } => None,
    };
    if target_schema.is_some_and(|schema| is_system_schema(&schema)) {
        return Err(CassieError::InsufficientPrivilege);
    }
    let name = normalize_relation_name(raw, context)?;
    require_existing_schema(&name, catalog)?;
    Ok(name)
}

/// Refuses to place a new object in a schema that does not exist, so every
/// stored object stays reachable through `search_path` and removable by
/// `DROP SCHEMA`. `public` always exists.
fn require_existing_schema(name: &str, catalog: &Catalog) -> Result<(), CassieError> {
    let Some(database) = crate::catalog::relation_database_name(name) else {
        return Ok(());
    };
    let schema = crate::catalog::relation_schema_name(name);
    if schema.eq_ignore_ascii_case(DEFAULT_SCHEMA) || is_system_schema(&schema) {
        return Ok(());
    }
    let namespace = canonical_schema_name(&database, &schema);
    if catalog.namespace_exists(&namespace) {
        Ok(())
    } else {
        Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Schema,
            name: namespace,
        })
    }
}

/// Resolves an existing object of one kind (view, rollup, projection) through
/// `search_path`, falling back to [`normalize_relation_name`] when no schema
/// holds a match so not-found and `IF EXISTS` handling stay unchanged.
///
/// # Errors
///
/// Returns an error when the reference is malformed or cross-database.
pub fn resolve_existing_name(
    raw: &str,
    context: &BindingContext,
    exists: impl Fn(&str) -> bool,
) -> Result<String, CassieError> {
    if context.scopes_database_objects() {
        if let ParsedName::Unqualified(name) = parse_name(raw).map_err(CassieError::Planner)? {
            for schema in context.relation_search_order() {
                let candidate = if is_system_schema(schema) {
                    format!("{schema}.{name}")
                } else {
                    canonical_relation_name(&context.database, schema, &name)
                };
                if exists(&candidate) {
                    return Ok(candidate);
                }
            }
        }
    }
    normalize_relation_name(raw, context)
}

/// # Errors
///
/// Returns an error when the schema reference is malformed or cross-database.
pub fn normalize_schema_name(raw: &str, context: &BindingContext) -> Result<String, CassieError> {
    if !context.scopes_database_objects() {
        return match parse_name(raw).map_err(CassieError::Planner)? {
            ParsedName::Unqualified(schema) => Ok(schema),
            ParsedName::SchemaQualified { schema, name } => Ok(format!("{schema}.{name}")),
            ParsedName::DatabaseQualified { .. } => Err(CassieError::Unsupported(
                "cross-database schema references are not supported".to_string(),
            )),
        };
    }

    match parse_name(raw).map_err(CassieError::Planner)? {
        ParsedName::Unqualified(schema) => Ok(canonical_schema_name(&context.database, &schema)),
        ParsedName::SchemaQualified { schema, name } => {
            if !schema.eq_ignore_ascii_case(&context.database) {
                return Err(CassieError::Unsupported(
                    "cross-database schema references are not supported".to_string(),
                ));
            }
            Ok(canonical_schema_name(&schema, &name))
        }
        ParsedName::DatabaseQualified { .. } => Err(CassieError::Unsupported(
            "cross-database schema references are not supported".to_string(),
        )),
    }
}

/// # Errors
///
/// Returns an error when the database name is malformed.
pub fn normalize_database_name(raw: &str) -> Result<String, CassieError> {
    match parse_name(raw).map_err(CassieError::Planner)? {
        ParsedName::Unqualified(name) => Ok(name),
        _ => Err(CassieError::Planner(
            "database names cannot be qualified".to_string(),
        )),
    }
}

/// # Errors
///
/// Returns an error when the relation does not exist or uses an unsupported qualifier depth.
pub fn resolve_relation_name(
    raw: &str,
    catalog: &crate::catalog::Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    let parsed = parse_name(raw).map_err(CassieError::Planner)?;
    let resolved = if context.scopes_database_objects() {
        resolve_scoped_relation_name(parsed, catalog, context)?
    } else {
        resolve_unscoped_relation_name(parsed, catalog, context)?
    };
    if crate::catalog::virtual_views::schema(&resolved).is_some() {
        return Ok(resolved);
    }
    // Resolution matches names without regard to case; hand back the stored
    // name so storage and catalog operations that use exact keys find it. A
    // reference that matches more than one stored relation is ambiguous rather
    // than resolved to an arbitrary one.
    let matches = catalog.matching_relation_names(&resolved);
    if matches.len() > 1 {
        return Err(CassieError::AmbiguousRelation(format!(
            "relation reference '{resolved}' is ambiguous between {}",
            matches.join(", ")
        )));
    }
    Ok(catalog.stored_relation_name(&resolved).unwrap_or(resolved))
}

fn resolve_unscoped_relation_name(
    parsed: ParsedName,
    catalog: &crate::catalog::Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    match parsed {
        ParsedName::Unqualified(name) => {
            for schema in context.relation_search_order() {
                let scoped_candidate = if is_system_schema(schema) {
                    format!("{schema}.{name}")
                } else {
                    canonical_relation_name(&context.database, schema, &name)
                };
                if catalog.relation_exists(&scoped_candidate)
                    || crate::catalog::virtual_views::schema(&scoped_candidate).is_some()
                {
                    return Ok(scoped_candidate);
                }
            }
            // Preserve support for catalogs populated directly by older callers while
            // preferring the canonical database-scoped name whenever it exists.
            if catalog.relation_exists(&name) {
                return Ok(name);
            }
            for system_schema in ["pg_catalog", "information_schema"] {
                let candidate = format!("{system_schema}.{name}");
                if crate::catalog::virtual_views::schema(&candidate).is_some() {
                    return Ok(candidate);
                }
            }
            Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name,
            })
        }
        ParsedName::SchemaQualified { schema, name } => {
            let candidate = if is_system_schema(&schema) {
                format!("{schema}.{name}")
            } else {
                canonical_relation_name(&context.database, &schema, &name)
            };
            if catalog.relation_exists(&candidate)
                || crate::catalog::virtual_views::schema(&candidate).is_some()
            {
                return Ok(candidate);
            }
            let legacy_candidate = format!("{schema}.{name}");
            if catalog.relation_exists(&legacy_candidate)
                || crate::catalog::virtual_views::schema(&legacy_candidate).is_some()
            {
                return Ok(legacy_candidate);
            }
            Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name: candidate,
            })
        }
        ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        } => {
            if !database.eq_ignore_ascii_case(&context.database) {
                return Err(CassieError::Unsupported(
                    "cross-database relation references are not supported".to_string(),
                ));
            }
            let candidate = canonical_relation_name(&database, &schema, &name);
            if catalog.relation_exists(&candidate)
                || crate::catalog::virtual_views::schema(&candidate).is_some()
            {
                return Ok(candidate);
            }
            Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name: candidate,
            })
        }
    }
}

fn resolve_scoped_relation_name(
    parsed: ParsedName,
    catalog: &crate::catalog::Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    match parsed {
        ParsedName::Unqualified(name) => {
            for schema in context.relation_search_order() {
                let candidate = if is_system_schema(schema) {
                    format!("{schema}.{name}")
                } else {
                    canonical_relation_name(&context.database, schema, &name)
                };
                if catalog.relation_exists(&candidate)
                    || crate::catalog::virtual_views::schema(&candidate).is_some()
                {
                    return Ok(candidate);
                }
            }

            for system_schema in ["pg_catalog", "information_schema"] {
                let candidate = format!("{system_schema}.{name}");
                if crate::catalog::virtual_views::schema(&candidate).is_some() {
                    return Ok(candidate);
                }
            }

            Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name,
            })
        }
        ParsedName::SchemaQualified { schema, name } => {
            let candidate = if is_system_schema(&schema) {
                format!("{schema}.{name}")
            } else {
                canonical_relation_name(&context.database, &schema, &name)
            };
            if catalog.relation_exists(&candidate)
                || crate::catalog::virtual_views::schema(&candidate).is_some()
            {
                return Ok(candidate);
            }
            Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                name: candidate,
            })
        }
        ParsedName::DatabaseQualified { .. } => Err(CassieError::Unsupported(
            "cross-database relation references are not supported".to_string(),
        )),
    }
}

/// # Errors
///
/// Returns an error when the schema does not exist or uses an unsupported qualifier depth.
pub fn resolve_schema_name(
    raw: &str,
    catalog: &crate::catalog::Catalog,
    context: &BindingContext,
) -> Result<String, CassieError> {
    if !context.scopes_database_objects() {
        if let Some(stored) = catalog.stored_namespace_name(raw) {
            return Ok(stored);
        }
    }

    let name = normalize_schema_name(raw, context)?;
    if let Some(stored) = catalog.stored_namespace_name(&name) {
        return Ok(stored);
    }

    Err(CassieError::CatalogObjectNotFound {
        kind: CatalogObjectKind::Schema,
        name,
    })
}

#[cfg(test)]
mod tests {
    use super::{resolve_relation_name, BindingContext};
    use crate::app::CassieError;
    use crate::catalog::Catalog;
    use crate::types::DataType;

    fn catalog_with_collections(names: &[&str]) -> Catalog {
        let catalog = Catalog::new();
        for name in names {
            catalog.register_collection(name, vec![("title".to_string(), DataType::Text)]);
        }
        catalog
    }

    #[test]
    fn should_reject_an_unqualified_relation_matching_two_schemas() {
        // Arrange
        let catalog = catalog_with_collections(&["postgres.a.orders", "postgres.b.orders"]);
        let context = BindingContext::unscoped("postgres", vec!["public".to_string()]);

        // Act
        let resolved = resolve_relation_name("orders", &catalog, &context);

        // Assert
        assert!(
            matches!(&resolved, Err(CassieError::AmbiguousRelation(message))
                if message.contains("postgres.a.orders") && message.contains("postgres.b.orders")),
            "ambiguous relation reference was not rejected: {resolved:?}"
        );
    }

    #[test]
    fn should_resolve_an_unqualified_relation_matching_one_schema() {
        // Arrange
        let catalog = catalog_with_collections(&["postgres.a.orders", "postgres.b.invoices"]);
        let context = BindingContext::unscoped("postgres", vec!["public".to_string()]);

        // Act
        let resolved = resolve_relation_name("orders", &catalog, &context);

        // Assert
        assert_eq!(resolved.ok().as_deref(), Some("postgres.a.orders"));
    }
}
