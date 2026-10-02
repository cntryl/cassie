use super::{CassieSession, Catalog};

#[derive(Debug, Clone)]
pub(super) struct SessionSchemaContext {
    catalog: Catalog,
    default_database: String,
}

pub(crate) fn resolved_search_path(
    catalog: &Catalog,
    database: &str,
    session: &CassieSession,
) -> Vec<String> {
    session
        .search_path()
        .into_iter()
        .map(|schema| {
            if schema == super::USER_SEARCH_PATH_ENTRY {
                session.user.clone()
            } else {
                schema
            }
        })
        .filter(|schema| {
            crate::catalog::is_system_schema(schema)
                || schema.eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA)
                || catalog
                    .namespace_exists(&crate::catalog::canonical_schema_name(database, schema))
        })
        .collect()
}

impl CassieSession {
    pub(crate) fn attach_schema_catalog(&self, catalog: &Catalog, default_database: &str) {
        *self.schema_context.lock() = Some(SessionSchemaContext {
            catalog: catalog.clone(),
            default_database: default_database.to_string(),
        });
    }

    pub(crate) fn current_existing_schema(&self) -> Option<String> {
        let context = self.schema_context.lock();
        context.as_ref().map_or_else(
            || {
                self.search_path()
                    .into_iter()
                    .find(|schema| schema != super::USER_SEARCH_PATH_ENTRY)
            },
            |scope| {
                resolved_search_path(
                    &scope.catalog,
                    self.current_database().unwrap_or(&scope.default_database),
                    self,
                )
                .into_iter()
                .next()
            },
        )
    }
}
