use super::{CassieSession, Catalog};
use crate::catalog::{RoleGrantTarget, RoleObjectPrivilege};

#[derive(Clone)]
pub(super) struct SessionSchemaContext {
    catalog: Catalog,
    write_catalog: Catalog,
    publication_catalog: Catalog,
    owner: Option<std::sync::Arc<crate::catalog::ControlledCatalogSnapshot>>,
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
    pub(crate) fn attach_schema_context(&self, app: &super::Cassie) {
        *self.schema_context.lock() = Some(SessionSchemaContext {
            catalog: app.catalog.clone(),
            write_catalog: app.metadata_write_catalog.clone(),
            publication_catalog: app.publication_catalog.clone(),
            owner: app.catalog_owner.clone(),
            default_database: app.default_database.clone(),
            midge: std::sync::Arc::downgrade(&app.midge),
        });
    }

    /// A transaction's private read aliases must not keep its catalog lease
    /// alive after the transaction has retired.
    pub(crate) fn retire_schema_context(&self) {
        if let Some(scope) = self.schema_context.lock().as_mut() {
            scope.catalog = scope.publication_catalog.clone();
            scope.write_catalog = scope.publication_catalog.clone();
            scope.owner = None;
        }
    }

    pub(crate) fn publish_sequence_metadata(&self, metadata: crate::catalog::SequenceMeta) {
        let catalog = self
            .schema_context
            .lock()
            .as_ref()
            .map(|scope| scope.write_catalog.clone());
        if let Some(catalog) = catalog {
            catalog.register_sequence(metadata);
        }
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

    pub(crate) fn has_database_privilege(
        &self,
        role_name: &str,
        database_name: &str,
        privilege: &str,
    ) -> Result<bool, super::CassieError> {
        if !privilege.eq_ignore_ascii_case("connect") {
            return Err(super::CassieError::Unsupported(format!(
                "database privilege '{privilege}' is outside the selected profile"
            )));
        }
        let catalog = self.privilege_catalog()?;
        let database = catalog
            .list_databases()
            .into_iter()
            .find(|database| database.name.eq_ignore_ascii_case(database_name))
            .ok_or_else(|| super::CassieError::CatalogObjectNotFound {
                kind: super::CatalogObjectKind::Database,
                name: database_name.to_owned(),
            })?;
        let role = privilege_role(&catalog, role_name, self)?;
        Ok(role.can_access_database(&database.name))
    }

    pub(crate) fn has_schema_privilege(
        &self,
        role_name: &str,
        schema_name: &str,
        privilege: &str,
    ) -> Result<bool, super::CassieError> {
        if !privilege.eq_ignore_ascii_case("usage") {
            return Err(super::CassieError::Unsupported(format!(
                "schema privilege '{privilege}' is outside the selected profile"
            )));
        }
        let (catalog, current_database) = self.privilege_catalog_and_database()?;
        let database = find_privilege_database(&catalog, &current_database)?;
        let schema = catalog
            .get_namespace(&crate::catalog::canonical_schema_name(
                &database.name,
                schema_name,
            ))
            .ok_or_else(|| super::CassieError::CatalogObjectNotFound {
                kind: super::CatalogObjectKind::Schema,
                name: schema_name.to_owned(),
            })?;
        let role = privilege_role(&catalog, role_name, self)?;
        if role.is_admin {
            return Ok(true);
        }
        Ok(catalog
            .get_role_object_grant(
                &database.physical_family,
                role.object_id,
                &RoleGrantTarget::Schema(schema.object_id),
            )
            .is_some_and(|grant| grant.grants(RoleObjectPrivilege::Usage)))
    }

    pub(crate) fn has_table_privilege(
        &self,
        role_name: &str,
        table_name: &str,
        privilege: &str,
    ) -> Result<bool, super::CassieError> {
        let privilege = match privilege.to_ascii_lowercase().as_str() {
            "select" => RoleObjectPrivilege::Select,
            "insert" => RoleObjectPrivilege::Insert,
            "update" => RoleObjectPrivilege::Update,
            "delete" => RoleObjectPrivilege::Delete,
            _ => {
                return Err(super::CassieError::Unsupported(format!(
                    "table privilege '{privilege}' is outside the selected profile"
                )));
            }
        };
        let (catalog, current_database) = self.privilege_catalog_and_database()?;
        let database = find_privilege_database(&catalog, &current_database)?;
        let search_path = resolved_search_path(&catalog, &database.name, self);
        let binding =
            crate::sql::binder::BindingContext::scoped(database.name.clone(), search_path);
        let role = privilege_role(&catalog, role_name, self)?;
        if catalog.all_indexes_snapshot().iter().any(|index| {
            crate::catalog::name_matches(&index.name, table_name)
                && crate::catalog::relation_database_name(&index.collection)
                    .is_none_or(|name| name.eq_ignore_ascii_case(&database.name))
        }) {
            return Ok(role.is_admin);
        }
        let resolved = crate::sql::binder::resolve_relation_name(table_name, &catalog, &binding)?;
        let Some(table) = catalog.get_collection(&resolved) else {
            if catalog.relation_exists(&resolved) {
                return Ok(role.is_admin);
            }
            return Err(super::CassieError::CatalogObjectNotFound {
                kind: super::CatalogObjectKind::Relation,
                name: table_name.to_owned(),
            });
        };
        if role.is_admin {
            return Ok(true);
        }
        Ok(catalog
            .get_role_object_grant(
                &database.physical_family,
                role.object_id,
                &RoleGrantTarget::Table(table.storage_id),
            )
            .is_some_and(|grant| grant.grants(privilege)))
    }

    fn privilege_catalog(&self) -> Result<Catalog, super::CassieError> {
        self.schema_context
            .lock()
            .as_ref()
            .map(|scope| scope.catalog.clone())
            .ok_or_else(|| {
                super::CassieError::Execution(
                    "catalog access is unavailable for privilege inspection".to_owned(),
                )
            })
    }

    fn privilege_catalog_and_database(&self) -> Result<(Catalog, String), super::CassieError> {
        let context = self.schema_context.lock();
        let scope = context.as_ref().ok_or_else(|| {
            super::CassieError::Execution(
                "catalog access is unavailable for privilege inspection".to_owned(),
            )
        })?;
        Ok((
            scope.catalog.clone(),
            self.current_database()
                .unwrap_or(&scope.default_database)
                .to_owned(),
        ))
    }
}

fn find_privilege_database(
    catalog: &Catalog,
    name: &str,
) -> Result<crate::catalog::DatabaseMeta, super::CassieError> {
    catalog
        .list_databases()
        .into_iter()
        .find(|database| database.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| super::CassieError::CatalogObjectNotFound {
            kind: super::CatalogObjectKind::Database,
            name: name.to_owned(),
        })
}

fn privilege_role(
    catalog: &Catalog,
    name: &str,
    session: &CassieSession,
) -> Result<crate::catalog::RoleMeta, super::CassieError> {
    catalog
        .get_role(name)
        .or_else(|| {
            (!session.is_network_authenticated() && name.eq_ignore_ascii_case(&session.user))
                .then(|| crate::catalog::RoleMeta::bootstrap_admin(name, None))
        })
        .ok_or_else(|| super::CassieError::CatalogObjectNotFound {
            kind: super::CatalogObjectKind::Role,
            name: name.to_owned(),
        })
}

#[cfg(test)]
mod tests;
