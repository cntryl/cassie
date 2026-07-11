use super::{
    CassieError, ColumnFamilyHandle, FamilyScope, Midge, StorageFamily, TransactionMode,
    DEFAULT_FAMILY_NAME,
};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, local_name, parse_name,
    relation_database_name, ParsedName,
};
use cntryl_midge::ConflictPolicy;

impl Midge {
    pub(crate) fn canonical_collection_name(&self, collection: &str) -> String {
        match parse_name(collection) {
            Ok(ParsedName::Unqualified(name)) => canonical_relation_name(
                &self.default_database,
                crate::catalog::DEFAULT_SCHEMA,
                &name,
            ),
            Ok(ParsedName::SchemaQualified { schema, name }) => {
                if crate::catalog::is_system_schema(&schema) {
                    format!("{schema}.{name}")
                } else {
                    canonical_relation_name(&self.default_database, &schema, &name)
                }
            }
            Ok(ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            }) => canonical_relation_name(&database, &schema, &name),
            Err(_) => collection.to_string(),
        }
    }

    pub(crate) fn canonical_namespace_name(&self, namespace: &str) -> String {
        match parse_name(namespace) {
            Ok(ParsedName::Unqualified(name)) if is_system_schema(&name) => name,
            Ok(ParsedName::Unqualified(name)) => {
                canonical_schema_name(&self.default_database, &name)
            }
            Ok(ParsedName::SchemaQualified { schema, name }) => {
                canonical_schema_name(&schema, &name)
            }
            Ok(ParsedName::DatabaseQualified {
                database, schema, ..
            }) => canonical_schema_name(&database, &schema),
            Err(_) => namespace.to_string(),
        }
    }

    pub(crate) fn display_collection_name(&self, collection: &str) -> String {
        let canonical = self.canonical_collection_name(collection);
        if relation_database_name(&canonical)
            .is_some_and(|database| database.eq_ignore_ascii_case(&self.default_database))
        {
            local_name(&canonical)
        } else {
            canonical
        }
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn schema_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Schema], mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn data_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.database_tx(&self.default_database, mode)
    }

    /// Open a transaction against exactly one logical database's physical
    /// column family. Schema/catalog and temporary transactions remain separate
    /// and cannot be combined with this transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the database is absent or its family cannot be
    /// resolved.
    pub fn database_tx(
        &self,
        database: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let family = self.database_family(database)?;
        self.engine
            .begin_tx(family.id(), mode)
            .map_err(CassieError::from)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn temp_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Temp], mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn default_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction_by_name(DEFAULT_FAMILY_NAME, mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn begin_families_tx(
        &self,
        families: &[StorageFamily],
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let scope = FamilyScope::for_families(families)?;
        let family = scope.family().ok_or_else(|| {
            CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            )
        })?;

        self.transaction(family, mode)
    }

    pub(crate) fn database_tx_for_collection(
        &self,
        collection: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        self.database_tx(&database, mode)
    }

    pub(super) fn transaction(
        &self,
        family: StorageFamily,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        match family {
            StorageFamily::Schema => self
                .engine
                .begin_tx(layout.schema.id(), mode)
                .map_err(CassieError::from),
            StorageFamily::Data => self.database_tx(&self.default_database, mode),
            StorageFamily::Temp => self
                .engine
                .begin_tx(layout.temp.id(), mode)
                .map_err(CassieError::from),
        }
    }

    pub(super) fn transaction_by_name(
        &self,
        family: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let Some(cf) = self.engine.get_column_family(family) else {
            return Err(CassieError::StorageMissingFamily(format!(
                "required column family '{family}' is missing"
            )));
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    pub(super) fn get_or_create_family(
        &self,
        family: StorageFamily,
    ) -> Result<ColumnFamilyHandle, CassieError> {
        let name = family.name();
        if let Some(existing) = self.engine.get_column_family(name) {
            return Ok(existing);
        }

        if let Ok(created) = self.engine.create_column_family(name) {
            return Ok(created);
        }

        self.engine.get_column_family(name).ok_or_else(|| {
            CassieError::StorageBootstrap(format!("cannot resolve required column family '{name}'"))
        })
    }

    pub(super) fn begin_schema_readonly_tx(
        &self,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadOnly)
    }

    pub(super) fn begin_schema_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadWrite)
    }

    pub(super) fn begin_data_readonly_tx_for(
        &self,
        collection: &str,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.database_tx_for_collection(collection, TransactionMode::ReadOnly)
    }

    pub(super) fn begin_data_rw_tx_for(
        &self,
        collection: &str,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let mut tx = self.database_tx_for_collection(collection, TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        Ok(tx)
    }
}
