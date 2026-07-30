use super::{Cassie, CassieError, CatalogObjectKind};
use crate::catalog::{canonical_schema_name, DatabaseMeta, DEFAULT_SCHEMA};

impl Cassie {
    pub(crate) fn create_logical_database(
        &self,
        name: &str,
        if_not_exists: bool,
    ) -> Result<DatabaseMeta, CassieError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CassieError::InvalidQuery(
                "database name cannot be empty".to_string(),
            ));
        }

        if let Some(database) = self.midge.get_database(name)? {
            if if_not_exists {
                return Ok(database);
            }
            return Err(CassieError::CatalogObjectAlreadyExists {
                kind: CatalogObjectKind::Database,
                name: database.name,
            });
        }

        self.midge.create_database(name, None)?;
        let database = self.midge.get_database(name)?.ok_or_else(|| {
            CassieError::StorageRetryable(format!(
                "database '{name}' metadata was unavailable after creation"
            ))
        })?;
        let public_schema = canonical_schema_name(&database.name, DEFAULT_SCHEMA);
        self.midge.create_namespace(&public_schema)?;
        self.catalog.register_database_metadata(database.clone());
        self.catalog.register_namespace(&public_schema, None);
        self.bump_schema_epoch_and_invalidate_query_cache()?;

        Ok(database)
    }
}
