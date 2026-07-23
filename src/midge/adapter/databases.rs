use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    collect_scan, key_encoding, CassieError, ColumnFamilyHandle, DatabaseMeta, Midge, Query,
    RawStorageEntry, TransactionMode, WriteOptions,
};

/// The resolved physical owner of one logical database.
#[derive(Debug, Clone)]
pub struct DatabaseFamily {
    pub metadata: DatabaseMeta,
    pub handle: ColumnFamilyHandle,
}

#[derive(Debug, Clone)]
pub(crate) struct StagedDatabaseFamily {
    pub(crate) metadata: DatabaseMeta,
    pub(crate) handle: ColumnFamilyHandle,
    operation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum DatabaseLifecycleKind {
    Create,
    Drop,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabaseLifecycleRecord {
    operation_id: String,
    kind: DatabaseLifecycleKind,
    name: String,
    physical_family: String,
}

impl Midge {
    /// Create the initial logical database without recursing through the public
    /// `ensure_families_ready` path. This is called while the baseline fixed families
    /// are being bootstrapped.
    pub(super) fn ensure_default_database(
        &self,
        schema: &ColumnFamilyHandle,
    ) -> Result<DatabaseFamily, CassieError> {
        let mut tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadWrite)?;
        if let Some(metadata) = Self::load_database_from_tx(&tx, &self.default_database)? {
            drop(tx);
            return self.resolve_database_family(&metadata);
        }

        let metadata = DatabaseMeta::new(&self.default_database, None);
        let record = DatabaseLifecycleRecord {
            operation_id: Uuid::new_v4().to_string(),
            kind: DatabaseLifecycleKind::Create,
            name: metadata.name.clone(),
            physical_family: metadata.physical_family.clone(),
        };
        Self::write_lifecycle_record(&mut tx, &record)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        let handle = self.create_physical_family(&metadata.physical_family)?;
        let mut finalize = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadWrite)?;
        Self::write_database_metadata(&mut finalize, &metadata)?;
        Self::delete_lifecycle_record(&mut finalize, &record.operation_id)?;
        finalize
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(DatabaseFamily { metadata, handle })
    }

    /// Replay a database family lifecycle journal before the registry is used.
    pub(super) fn replay_database_lifecycle_operations(
        &self,
        schema: &ColumnFamilyHandle,
    ) -> Result<(), CassieError> {
        let tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadOnly)?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(key_encoding::database_lifecycle_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut records = Vec::new();
        for (_key, value) in entries {
            let record: DatabaseLifecycleRecord =
                serde_json::from_slice(&value).map_err(|error| {
                    CassieError::StorageBootstrap(format!(
                        "invalid database lifecycle journal record: {error}"
                    ))
                })?;
            records.push(record);
        }
        drop(tx);

        for record in records {
            match record.kind {
                DatabaseLifecycleKind::Create => {
                    let metadata = self.load_database_without_layout(&record.name, schema)?;
                    match metadata {
                        Some(metadata) => {
                            if metadata.physical_family != record.physical_family {
                                return Err(CassieError::StorageBootstrap(format!(
                                    "database '{}' lifecycle family mismatch",
                                    record.name
                                )));
                            }
                            if self
                                .engine
                                .get_column_family(&record.physical_family)
                                .is_none()
                            {
                                let _ = self.create_physical_family(&record.physical_family)?;
                            }
                        }
                        None => {
                            self.drop_physical_family_if_present(&record.physical_family)?;
                        }
                    }
                }
                DatabaseLifecycleKind::Drop => {
                    // A drop journal is only destructive once the catalog record
                    // is gone. If recovery finds it still present, preserve the
                    // active database and discard the stale journal.
                    if self
                        .load_database_without_layout(&record.name, schema)?
                        .is_none()
                    {
                        self.drop_physical_family_if_present(&record.physical_family)?;
                    }
                }
            }

            let mut cleanup =
                self.begin_schema_tx_for_handle(schema, TransactionMode::ReadWrite)?;
            Self::delete_lifecycle_record(&mut cleanup, &record.operation_id)?;
            cleanup
                .commit(WriteOptions::sync())
                .map_err(CassieError::from)?;
        }
        Ok(())
    }

    pub(super) fn load_database_families(
        &self,
        schema: &ColumnFamilyHandle,
    ) -> Result<BTreeMap<String, DatabaseFamily>, CassieError> {
        let tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadOnly)?;
        let mut names = Self::load_databases(&tx)?;
        if names.is_empty() {
            let scan = collect_scan(
                tx.scan(&Query::new().prefix(Self::database_prefix().into()))
                    .map_err(CassieError::from)?,
            )?;
            for (_key, raw_value) in scan {
                if let Ok(metadata) = serde_json::from_slice::<DatabaseMeta>(&raw_value) {
                    names.push(metadata.name);
                }
            }
        }

        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        let mut families = BTreeMap::new();
        for name in names {
            let Some(metadata) = Self::load_database_from_tx(&tx, &name)? else {
                return Err(CassieError::StorageBootstrap(format!(
                    "database registry references missing database '{name}'"
                )));
            };
            let family = self.resolve_database_family(&metadata)?;
            let normalized = metadata.name.to_ascii_lowercase();
            if families.insert(normalized, family).is_some() {
                return Err(CassieError::StorageBootstrap(format!(
                    "duplicate database registry entry '{name}'"
                )));
            }
        }
        let registered_families = families
            .values()
            .map(|family| family.metadata.physical_family.clone())
            .collect::<std::collections::HashSet<_>>();
        for family in self
            .engine
            .list_column_families()
            .map_err(CassieError::from)?
        {
            if family.name().starts_with("db-") && !registered_families.contains(family.name()) {
                return Err(CassieError::StorageBootstrap(format!(
                    "orphan database column family '{}' is not registered",
                    family.name()
                )));
            }
        }
        Ok(families)
    }

    /// # Errors
    ///
    /// Returns an error if the database is absent or its physical family is
    /// missing from the Midge manifest.
    pub fn database_family(&self, database: &str) -> Result<ColumnFamilyHandle, CassieError> {
        self.ensure_families_ready()?;
        self.database_families
            .read()
            .get(&database.to_ascii_lowercase())
            .map(|family| family.handle.clone())
            .ok_or_else(|| CassieError::NotFound(format!("database '{database}' does not exist")))
    }

    pub(crate) fn database_metadata(
        &self,
        database: &str,
    ) -> Result<Option<DatabaseMeta>, CassieError> {
        self.ensure_families_ready()?;
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_database_from_tx(&tx, database)
    }

    /// # Errors
    ///
    /// Returns an error if the database already exists, the lifecycle journal
    /// cannot be persisted, or its physical family cannot be created.
    pub fn create_database(
        &self,
        name: &str,
        description: Option<String>,
    ) -> Result<(), CassieError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CassieError::InvalidQuery(
                "database name cannot be empty".to_string(),
            ));
        }
        let schema = self.ensure_families_ready()?.schema.clone();
        let mut tx = self.begin_schema_tx_for_handle(&schema, TransactionMode::ReadWrite)?;
        if let Some(metadata) = Self::load_database_from_tx(&tx, name)? {
            drop(tx);
            let _ = self.resolve_database_family(&metadata)?;
            return Ok(());
        }

        let metadata = DatabaseMeta::new(name, description);
        let record = DatabaseLifecycleRecord {
            operation_id: Uuid::new_v4().to_string(),
            kind: DatabaseLifecycleKind::Create,
            name: metadata.name.clone(),
            physical_family: metadata.physical_family.clone(),
        };
        Self::write_lifecycle_record(&mut tx, &record)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;

        let handle = self.create_physical_family(&metadata.physical_family)?;
        let mut finalize = self.begin_schema_tx_for_handle(&schema, TransactionMode::ReadWrite)?;
        Self::write_database_metadata(&mut finalize, &metadata)?;
        Self::delete_lifecycle_record(&mut finalize, &record.operation_id)?;
        finalize
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.database_families.write().insert(
            metadata.name.to_ascii_lowercase(),
            DatabaseFamily { metadata, handle },
        );
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error if the database is not empty or its lifecycle cannot be
    /// completed atomically enough for startup recovery to finish it.
    pub fn drop_database(&self, name: &str) -> Result<(), CassieError> {
        let schema = self.ensure_families_ready()?.schema.clone();
        let metadata = self
            .database_metadata(name)?
            .ok_or_else(|| CassieError::NotFound(format!("database '{name}' does not exist")))?;
        let handle = self.resolve_database_family(&metadata)?.handle;
        let data_tx = self
            .engine
            .begin_tx(handle.id(), TransactionMode::ReadOnly)
            .map_err(CassieError::from)?;
        let has_data = collect_scan(data_tx.scan(&Query::new()).map_err(CassieError::from)?)?
            .into_iter()
            .any(|(key, _)| is_database_data_key(&key));
        if has_data {
            return Err(CassieError::Unsupported(format!(
                "database '{}' is not empty",
                metadata.name
            )));
        }
        if self
            .database_catalog_entries(name)?
            .iter()
            .any(|(key, _)| is_database_catalog_data_key(key))
        {
            return Err(CassieError::Unsupported(format!(
                "database '{}' is not empty",
                metadata.name
            )));
        }
        drop(data_tx);

        let record = DatabaseLifecycleRecord {
            operation_id: Uuid::new_v4().to_string(),
            kind: DatabaseLifecycleKind::Drop,
            name: metadata.name.clone(),
            physical_family: metadata.physical_family.clone(),
        };
        let mut journal = self.begin_schema_tx_for_handle(&schema, TransactionMode::ReadWrite)?;
        Self::write_lifecycle_record(&mut journal, &record)?;
        journal
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut catalog = self.begin_schema_tx_for_handle(&schema, TransactionMode::ReadWrite)?;
        catalog
            .delete(Self::database_key(&metadata.name))
            .map_err(CassieError::from)?;
        let mut databases = Self::load_databases(&catalog)?;
        databases.retain(|entry| !entry.eq_ignore_ascii_case(&metadata.name));
        Self::save_databases(&mut catalog, &databases)?;
        catalog
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        self.engine
            .drop_column_family(handle.id())
            .map_err(CassieError::from)?;
        let mut cleanup = self.begin_schema_tx_for_handle(&schema, TransactionMode::ReadWrite)?;
        Self::delete_lifecycle_record(&mut cleanup, &record.operation_id)?;
        cleanup
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        self.database_families
            .write()
            .remove(&metadata.name.to_ascii_lowercase());
        Ok(())
    }

    pub(crate) fn database_catalog_entries(
        &self,
        database: &str,
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let entries = self.raw_scan_prefix(super::StorageFamily::Schema, b"")?;
        let mut scoped_entries = Vec::new();
        for (key, value) in entries {
            if !catalog_entry_belongs_to_database(&key, &value, database) {
                continue;
            }
            if matches!(key_family(&key), Some("collections" | "namespaces")) {
                let values: Vec<String> = serde_json::from_slice(&value).map_err(|error| {
                    CassieError::Parse(format!("invalid database catalog list: {error}"))
                })?;
                let values = values
                    .into_iter()
                    .filter(|value| catalog_name_belongs_to_database(value, database))
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    continue;
                }
                let value = serde_json::to_vec(&values)
                    .map_err(|error| CassieError::Parse(error.to_string()))?;
                scoped_entries.push((key, value));
            } else {
                scoped_entries.push((key, value));
            }
        }
        Ok(scoped_entries)
    }

    pub(crate) fn stage_database_family(
        &self,
        name: &str,
    ) -> Result<StagedDatabaseFamily, CassieError> {
        self.ensure_families_ready()?;
        if self.get_database(name)?.is_some() {
            return Err(CassieError::Unsupported(format!(
                "database '{name}' already exists"
            )));
        }
        let metadata = DatabaseMeta::new(name, None);
        let operation_id = Uuid::new_v4().to_string();
        let record = DatabaseLifecycleRecord {
            operation_id: operation_id.clone(),
            kind: DatabaseLifecycleKind::Create,
            name: metadata.name.clone(),
            physical_family: metadata.physical_family.clone(),
        };
        let mut tx = self.begin_schema_rw_tx()?;
        Self::write_lifecycle_record(&mut tx, &record)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        let handle = self.create_physical_family(&metadata.physical_family)?;
        Ok(StagedDatabaseFamily {
            metadata,
            handle,
            operation_id,
        })
    }

    pub(crate) fn write_staged_database_entries(
        &self,
        staged: &StagedDatabaseFamily,
        entries: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<(), CassieError> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .engine
            .begin_tx(staged.handle.id(), TransactionMode::ReadWrite)
            .map_err(CassieError::from)?;
        for (key, value) in entries {
            tx.put(key.clone(), value.clone(), None)
                .map_err(CassieError::from)?;
        }
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    pub(crate) fn commit_staged_database_family(
        &self,
        staged: StagedDatabaseFamily,
        source_database: &str,
        source_physical_family: &str,
        catalog_entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), CassieError> {
        let cleanup = staged.clone();
        let result = self.commit_staged_database_family_inner(
            staged,
            source_database,
            source_physical_family,
            catalog_entries,
        );
        if result.is_err() {
            let _ = self.abort_staged_database_family(&cleanup);
        }
        result
    }

    fn commit_staged_database_family_inner(
        &self,
        staged: StagedDatabaseFamily,
        source_database: &str,
        source_physical_family: &str,
        catalog_entries: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let mut collection_names = Vec::new();
        let mut namespace_names = Vec::new();
        for (key, value) in catalog_entries {
            validate_database_catalog_entry(&key, &value, source_database)?;
            let Some(family) = key_family(&key) else {
                return Err(CassieError::Parse(
                    "database image contains an invalid catalog key".to_string(),
                ));
            };
            match family {
                "layout" | "database-lifecycle" | "schema-epoch" | "databases" | "database" => {
                    continue
                }
                "collections" => {
                    collection_names.extend(rewrite_string_list(
                        &value,
                        source_database,
                        &staged.metadata.name,
                    )?);
                    continue;
                }
                "namespaces" => {
                    namespace_names.extend(rewrite_string_list(
                        &value,
                        source_database,
                        &staged.metadata.name,
                    )?);
                    continue;
                }
                _ => {}
            }
            let key = rewrite_key_component(&key, source_database, &staged.metadata.name);
            let value = rewrite_json_value(
                &value,
                source_database,
                &staged.metadata.name,
                source_physical_family,
                &staged.metadata.physical_family,
            )?;
            tx.put(key, value, None).map_err(CassieError::from)?;
        }

        Self::write_database_metadata(&mut tx, &staged.metadata)?;
        merge_string_list(&mut tx, Self::collections_key(), collection_names)?;
        merge_string_list(&mut tx, Self::namespaces_key(), namespace_names)?;
        Self::delete_lifecycle_record(&mut tx, &staged.operation_id)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        self.database_families.write().insert(
            staged.metadata.name.to_ascii_lowercase(),
            DatabaseFamily {
                metadata: staged.metadata,
                handle: staged.handle,
            },
        );
        Ok(())
    }

    pub(crate) fn abort_staged_database_family(
        &self,
        staged: &StagedDatabaseFamily,
    ) -> Result<(), CassieError> {
        self.drop_physical_family_if_present(&staged.metadata.physical_family)?;
        let mut tx = self.begin_schema_rw_tx()?;
        Self::delete_lifecycle_record(&mut tx, &staged.operation_id)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    fn resolve_database_family(
        &self,
        metadata: &DatabaseMeta,
    ) -> Result<DatabaseFamily, CassieError> {
        if metadata.physical_family == super::DEFAULT_FAMILY_NAME
            || metadata.physical_family == super::SCHEMA_FAMILY_NAME
            || metadata.physical_family == super::TEMP_FAMILY_NAME
        {
            return Err(CassieError::StorageBootstrap(format!(
                "database '{}' uses reserved physical family '{}'",
                metadata.name, metadata.physical_family
            )));
        }
        let handle = self
            .engine
            .get_column_family(&metadata.physical_family)
            .ok_or_else(|| {
                CassieError::StorageMissingFamily(format!(
                    "database '{}' requires missing column family '{}'",
                    metadata.name, metadata.physical_family
                ))
            })?;
        Ok(DatabaseFamily {
            metadata: metadata.clone(),
            handle,
        })
    }

    fn create_physical_family(&self, name: &str) -> Result<ColumnFamilyHandle, CassieError> {
        if let Some(handle) = self.engine.get_column_family(name) {
            return Ok(handle);
        }
        self.engine
            .create_column_family(name)
            .or_else(|_| {
                self.engine.get_column_family(name).ok_or_else(|| {
                    cntryl_midge::MidgeError::InvalidArgument(format!(
                        "cannot create or resolve database family '{name}'"
                    ))
                })
            })
            .map_err(CassieError::from)
    }

    fn drop_physical_family_if_present(&self, name: &str) -> Result<(), CassieError> {
        if let Some(handle) = self.engine.get_column_family(name) {
            self.engine
                .drop_column_family(handle.id())
                .map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn begin_schema_tx_for_handle(
        &self,
        schema: &ColumnFamilyHandle,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.engine
            .begin_tx(schema.id(), mode)
            .map_err(CassieError::from)
    }

    fn load_database_without_layout(
        &self,
        name: &str,
        schema: &ColumnFamilyHandle,
    ) -> Result<Option<DatabaseMeta>, CassieError> {
        let tx = self.begin_schema_tx_for_handle(schema, TransactionMode::ReadOnly)?;
        Self::load_database_from_tx(&tx, name)
    }

    fn write_database_metadata(
        tx: &mut cntryl_midge::Transaction,
        metadata: &DatabaseMeta,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::database_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        let mut databases = Self::load_databases(tx)?;
        if !databases
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(&metadata.name))
        {
            databases.push(metadata.name.clone());
            databases.sort_by_key(|entry| entry.to_ascii_lowercase());
            databases.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
            Self::save_databases(tx, &databases)?;
        }
        Ok(())
    }

    fn write_lifecycle_record(
        tx: &mut cntryl_midge::Transaction,
        record: &DatabaseLifecycleRecord,
    ) -> Result<(), CassieError> {
        let value =
            serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            key_encoding::database_lifecycle_key(&record.operation_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        Ok(())
    }

    fn delete_lifecycle_record(
        tx: &mut cntryl_midge::Transaction,
        operation_id: &str,
    ) -> Result<(), CassieError> {
        tx.delete(key_encoding::database_lifecycle_key(operation_id))
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the catalog value is malformed.
    pub(crate) fn load_database_from_tx(
        tx: &cntryl_midge::Transaction,
        name: &str,
    ) -> Result<Option<DatabaseMeta>, CassieError> {
        let Some(raw) = tx
            .get(&Self::database_key(name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid database metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when catalog storage cannot be read.
    pub fn get_database(&self, name: &str) -> Result<Option<DatabaseMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_database_from_tx(&tx, name)
    }

    /// # Errors
    ///
    /// Returns an error when catalog storage cannot be read.
    pub fn list_databases(&self) -> Result<Vec<DatabaseMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let mut names = Self::load_databases(&tx)?;
        if names.is_empty() {
            let scan = collect_scan(
                tx.scan(&Query::new().prefix(Self::database_prefix().into()))
                    .map_err(CassieError::from)?,
            )?;
            let prefix = Self::database_prefix();
            for (raw_key, raw_value) in scan {
                if let Ok(metadata) = serde_json::from_slice::<DatabaseMeta>(&raw_value) {
                    names.push(metadata.name);
                    continue;
                }
                if let Some(name) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) {
                    names.push(name);
                }
            }
        }

        names.sort_by_key(|name| name.to_ascii_lowercase());
        names.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        let mut databases = Vec::with_capacity(names.len());
        for name in names {
            if let Some(metadata) = Self::load_database_from_tx(&tx, &name)? {
                databases.push(metadata);
            }
        }
        databases.sort_by_key(|database| database.name.to_ascii_lowercase());
        Ok(databases)
    }
}

fn key_components(key: &[u8]) -> impl Iterator<Item = &[u8]> {
    key.split(|byte| *byte == cntryl_lexkey::LexKey::SEPARATOR)
}

fn key_family(key: &[u8]) -> Option<&str> {
    key_components(key)
        .nth(2)
        .and_then(|component| std::str::from_utf8(component).ok())
}

fn catalog_entry_belongs_to_database(key: &[u8], value: &[u8], database: &str) -> bool {
    let Some(family) = key_family(key) else {
        return false;
    };
    if matches!(family, "collections" | "namespaces") {
        return serde_json::from_slice::<Vec<String>>(value).is_ok_and(|values| {
            values
                .iter()
                .any(|value| catalog_name_belongs_to_database(value, database))
        });
    }
    is_database_scoped_catalog_family(family)
        && key_components(key)
            .nth(3)
            .is_some_and(|component| component.eq_ignore_ascii_case(database.as_bytes()))
}

pub(crate) fn validate_database_catalog_entry(
    key: &[u8],
    value: &[u8],
    database: &str,
) -> Result<(), CassieError> {
    let family = key_family(key).ok_or_else(|| {
        CassieError::Parse("database image contains an invalid catalog key".to_string())
    })?;
    if matches!(family, "collections" | "namespaces") {
        let values: Vec<String> = serde_json::from_slice(value).map_err(|error| {
            CassieError::Parse(format!("invalid database catalog list: {error}"))
        })?;
        if values
            .iter()
            .all(|value| catalog_name_belongs_to_database(value, database))
        {
            return Ok(());
        }
    } else if is_database_scoped_catalog_family(family)
        && key_components(key)
            .nth(3)
            .is_some_and(|component| component.eq_ignore_ascii_case(database.as_bytes()))
    {
        return Ok(());
    }
    Err(CassieError::Unsupported(format!(
        "database image catalog family '{family}' is not scoped to database '{database}'"
    )))
}

fn is_database_scoped_catalog_family(family: &str) -> bool {
    matches!(
        family,
        "schema"
            | "row-schema"
            | "projection"
            | "vector-index"
            | "index"
            | "view"
            | "sequence"
            | "constraints"
            | "namespace"
            | "cardinality"
            | "collection-meta"
            | "rollup"
            | "retention"
            | "collection-generation"
            | "maintenance-debt"
            | "graph"
    )
}

fn catalog_name_belongs_to_database(name: &str, database: &str) -> bool {
    let name = name.trim();
    name.eq_ignore_ascii_case(database)
        || name
            .get(..database.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(database))
            && name.as_bytes().get(database.len()) == Some(&b'.')
}

fn rewrite_key_component(key: &[u8], source: &str, target: &str) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(key.len() + target.len().saturating_sub(source.len()));
    for (index, component) in key_components(key).enumerate() {
        if index > 0 {
            rewritten.push(cntryl_lexkey::LexKey::SEPARATOR);
        }
        if component.eq_ignore_ascii_case(source.as_bytes()) {
            rewritten.extend_from_slice(target.as_bytes());
        } else {
            rewritten.extend_from_slice(component);
        }
    }
    rewritten
}

fn rewrite_json_value(
    raw: &[u8],
    source: &str,
    target: &str,
    source_physical: &str,
    target_physical: &str,
) -> Result<Vec<u8>, CassieError> {
    let mut value: serde_json::Value = serde_json::from_slice(raw).map_err(|error| {
        CassieError::Parse(format!("invalid database catalog image value: {error}"))
    })?;
    rewrite_json_strings(&mut value, source, target, source_physical, target_physical);
    serde_json::to_vec(&value).map_err(|error| CassieError::Parse(error.to_string()))
}

fn rewrite_json_strings(
    value: &mut serde_json::Value,
    source: &str,
    target: &str,
    source_physical: &str,
    target_physical: &str,
) {
    match value {
        serde_json::Value::String(text) => {
            *text = text
                .replace(source_physical, target_physical)
                .replace(source, target);
        }
        serde_json::Value::Array(values) => {
            for value in values {
                rewrite_json_strings(value, source, target, source_physical, target_physical);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                rewrite_json_strings(value, source, target, source_physical, target_physical);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn rewrite_string_list(raw: &[u8], source: &str, target: &str) -> Result<Vec<String>, CassieError> {
    let values: Vec<String> = serde_json::from_slice(raw)
        .map_err(|error| CassieError::Parse(format!("invalid database catalog list: {error}")))?;
    values
        .into_iter()
        .map(|value| {
            if !catalog_name_belongs_to_database(&value, source) {
                return Err(CassieError::Unsupported(format!(
                    "database image catalog name '{value}' is outside source database '{source}'"
                )));
            }
            Ok(rewrite_catalog_name(&value, source, target))
        })
        .collect()
}

fn rewrite_catalog_name(value: &str, source: &str, target: &str) -> String {
    value
        .get(source.len()..)
        .map_or_else(|| target.to_string(), |suffix| format!("{target}{suffix}"))
}

fn merge_string_list(
    tx: &mut cntryl_midge::Transaction,
    key: Vec<u8>,
    additions: Vec<String>,
) -> Result<(), CassieError> {
    let mut values = tx
        .get(&key)
        .map_err(CassieError::from)?
        .map_or_else(Vec::new, |raw| {
            serde_json::from_slice::<Vec<String>>(&raw).unwrap_or_default()
        });
    values.extend(additions);
    values.sort();
    values.dedup();
    tx.put(
        key,
        serde_json::to_vec(&values).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)
}

fn is_database_data_key(key: &[u8]) -> bool {
    matches!(
        key_family(key),
        Some(
            "row"
                | "legacy-doc"
                | "scalar-index"
                | "time-series-index"
                | "normalized-vector"
                | "vector-index-state"
                | "unique-reservation"
                | "column-batch"
                | "column-store"
                | "row-hash"
                | "range-hash"
                | "root-hash"
                | "graph-adjacency"
        )
    )
}

fn is_database_catalog_data_key(key: &[u8]) -> bool {
    match key_family(key) {
        Some("database" | "databases" | "collections" | "namespaces") | None => false,
        Some("namespace") => key_components(key)
            .nth(4)
            .is_none_or(|schema| !schema.eq_ignore_ascii_case(b"public")),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{catalog_entry_belongs_to_database, validate_database_catalog_entry};

    #[test]
    fn should_recognize_only_database_scoped_catalog_keys() {
        // Arrange
        let matching = super::super::key_encoding::collection_schema_key("analytics.public.docs");
        let foreign = super::super::key_encoding::collection_schema_key("other.public.docs");
        let role = super::super::key_encoding::role_key("injected_admin");

        // Act
        let matching_result = validate_database_catalog_entry(&matching, b"{}", "analytics");
        let foreign_result = validate_database_catalog_entry(&foreign, b"{}", "analytics");
        let role_result = validate_database_catalog_entry(&role, b"{}", "analytics");

        // Assert
        assert!(catalog_entry_belongs_to_database(
            &matching,
            b"{}",
            "analytics"
        ));
        assert!(matching_result.is_ok());
        assert!(foreign_result.is_err());
        assert!(role_result.is_err());
    }
}
