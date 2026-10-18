use super::{CassieSession, Catalog};

#[derive(Clone)]
pub(super) struct SessionSchemaContext {
    catalog: Catalog,
    default_database: String,
    /// Weak so a session that outlives its `Cassie` does not keep the
    /// storage (and its writer lease) open.
    midge: std::sync::Weak<crate::midge::Midge>,
}

impl std::fmt::Debug for SessionSchemaContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionSchemaContext")
            .field("default_database", &self.default_database)
            .finish_non_exhaustive()
    }
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
    /// Attaches the catalog and storage that session-evaluated functions such
    /// as `nextval` act on.
    pub(crate) fn attach_schema_context(
        &self,
        catalog: &Catalog,
        default_database: &str,
        midge: &std::sync::Arc<crate::midge::Midge>,
    ) {
        *self.schema_context.lock() = Some(SessionSchemaContext {
            catalog: catalog.clone(),
            default_database: default_database.to_string(),
            midge: std::sync::Arc::downgrade(midge),
        });
    }

    /// The catalog, binding context and storage for session sequence functions.
    pub(crate) fn sequence_context(
        &self,
    ) -> Option<(
        Catalog,
        crate::sql::binder::BindingContext,
        std::sync::Arc<crate::midge::Midge>,
    )> {
        let midge = self.schema_context.lock().as_ref()?.midge.upgrade()?;
        let (catalog, context) = self.object_lookup_context()?;
        Some((catalog, context, midge))
    }

    /// The catalog and binding context `reg*` lookups resolve names in.
    pub(crate) fn object_lookup_context(
        &self,
    ) -> Option<(Catalog, crate::sql::binder::BindingContext)> {
        let context = self.schema_context.lock();
        let scope = context.as_ref()?;
        let database = self
            .current_database()
            .unwrap_or(&scope.default_database)
            .to_string();
        let search_path = resolved_search_path(&scope.catalog, &database, self);
        Some((
            scope.catalog.clone(),
            crate::sql::binder::BindingContext::scoped(database, search_path),
        ))
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
