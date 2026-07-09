use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, parse_name, ParsedName,
    DEFAULT_SCHEMA,
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
    if !context.scopes_database_objects() {
        if catalog.relation_exists(raw) || crate::catalog::virtual_views::schema(raw).is_some() {
            return Ok(raw.to_string());
        }

        if let ParsedName::Unqualified(name) = &parsed {
            for schema in &context.search_path {
                let candidate = if is_system_schema(schema) {
                    format!("{schema}.{name}")
                } else {
                    format!("{schema}.{name}")
                };
                if catalog.relation_exists(&candidate)
                    || crate::catalog::virtual_views::schema(&candidate).is_some()
                {
                    return Ok(candidate);
                }
            }
        }

        return match parsed {
            ParsedName::Unqualified(name) => {
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
                let candidate = format!("{schema}.{name}");
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
        };
    }

    match parsed {
        ParsedName::Unqualified(name) => {
            for schema in &context.search_path {
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
    if !context.scopes_database_objects() && catalog.namespace_exists(raw) {
        return Ok(raw.to_string());
    }

    let name = normalize_schema_name(raw, context)?;
    if catalog.namespace_exists(&name) {
        return Ok(name);
    }

    Err(CassieError::CatalogObjectNotFound {
        kind: CatalogObjectKind::Schema,
        name,
    })
}
