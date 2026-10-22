use super::{
    collect_scan, CassieError, ColumnFamilyHandle, FamilyScope, Midge, Query, RawStorageEntry,
    StorageFamily, TransactionMode, WriteOptions, DEFAULT_FAMILY_NAME,
};
use crate::catalog::{
    canonical_relation_name, canonical_schema_name, is_system_schema, local_name, parse_name,
    relation_database_name, schema_database_name, ParsedName,
};
use cntryl_midge::ConflictPolicy;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StorageOverlay {
    pub(crate) changes: BTreeMap<Vec<u8>, StorageMutation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExpectedStorageValue {
    Untracked,
    Missing,
    Present(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageMutation {
    pub(crate) value: Option<Vec<u8>>,
    pub(crate) ttl_seconds: Option<u64>,
    /// The value observed before this transaction first changed the key.
    pub(crate) expected: ExpectedStorageValue,
}

#[derive(Clone)]
struct ActiveStorageScope {
    midge_identity: usize,
    family_id: cntryl_midge::ColumnFamilyId,
    overlay: Arc<Mutex<StorageOverlay>>,
    stage_writes: bool,
}

thread_local! {
    static ACTIVE_STORAGE_SCOPE: RefCell<Option<ActiveStorageScope>> = const { RefCell::new(None) };
}

pub(crate) struct StorageScopeGuard {
    previous: Option<ActiveStorageScope>,
    _thread: PhantomData<Rc<()>>,
}

impl StorageScopeGuard {
    fn enter(scope: ActiveStorageScope) -> Self {
        let previous = ACTIVE_STORAGE_SCOPE.with(|current| current.replace(Some(scope)));
        Self {
            previous,
            _thread: PhantomData,
        }
    }
}

impl Drop for StorageScopeGuard {
    fn drop(&mut self) {
        ACTIVE_STORAGE_SCOPE.with(|current| current.replace(self.previous.take()));
    }
}

pub(crate) struct AdapterTransaction {
    inner: Option<cntryl_midge::Transaction>,
    overlay: Option<Arc<Mutex<StorageOverlay>>>,
    stage_writes: bool,
    suppress_commit: bool,
    local_changes: BTreeMap<Vec<u8>, StorageMutation>,
}

impl AdapterTransaction {
    fn raw(inner: cntryl_midge::Transaction) -> Self {
        Self {
            inner: Some(inner),
            overlay: None,
            stage_writes: false,
            suppress_commit: false,
            local_changes: BTreeMap::new(),
        }
    }

    fn staged(
        inner: cntryl_midge::Transaction,
        overlay: Arc<Mutex<StorageOverlay>>,
        stage_writes: bool,
        suppress_commit: bool,
    ) -> Self {
        Self {
            inner: Some(inner),
            overlay: Some(overlay),
            stage_writes,
            suppress_commit,
            local_changes: BTreeMap::new(),
        }
    }

    fn inner(&self) -> &cntryl_midge::Transaction {
        self.inner
            .as_ref()
            .expect("transaction is consumed only at commit")
    }

    fn inner_mut(&mut self) -> &mut cntryl_midge::Transaction {
        self.inner
            .as_mut()
            .expect("transaction is consumed only at commit")
    }

    fn expected_value_before_change(
        &self,
        key: &[u8],
    ) -> Result<ExpectedStorageValue, cntryl_midge::MidgeError> {
        if let Some(change) = self.local_changes.get(key) {
            return Ok(change.expected.clone());
        }
        if let Some(change) = self
            .overlay
            .as_ref()
            .and_then(|overlay| overlay.lock().changes.get(key).cloned())
        {
            return Ok(change.expected);
        }
        Ok(self
            .inner()
            .get(key)?
            .map_or(ExpectedStorageValue::Missing, |value| {
                ExpectedStorageValue::Present(value.to_vec())
            }))
    }

    fn record_change(
        &mut self,
        key: Vec<u8>,
        value: Option<Vec<u8>>,
        ttl_seconds: Option<u64>,
        expected: ExpectedStorageValue,
    ) {
        self.local_changes.insert(
            key,
            StorageMutation {
                value,
                ttl_seconds,
                expected,
            },
        );
    }

    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<bytes::Bytes>, cntryl_midge::MidgeError> {
        self.inner().get(key)
    }

    pub(crate) fn put(
        &mut self,
        key: Vec<u8>,
        value: Vec<u8>,
        ttl_seconds: Option<u64>,
    ) -> Result<(), cntryl_midge::MidgeError> {
        let expected = if self.stage_writes {
            self.expected_value_before_change(&key)?
        } else {
            ExpectedStorageValue::Untracked
        };
        self.inner_mut()
            .put(key.clone(), value.clone(), ttl_seconds)?;
        if self.stage_writes {
            self.record_change(key, Some(value), ttl_seconds, expected);
        }
        Ok(())
    }

    pub(crate) fn delete(&mut self, key: Vec<u8>) -> Result<(), cntryl_midge::MidgeError> {
        let expected = if self.stage_writes {
            self.expected_value_before_change(&key)?
        } else {
            ExpectedStorageValue::Untracked
        };
        self.inner_mut().delete(key.clone())?;
        if self.stage_writes {
            self.record_change(key, None, None, expected);
        }
        Ok(())
    }

    pub(crate) fn scan(
        &self,
        query: &Query,
    ) -> Result<cntryl_midge::ScanIterator<'_>, cntryl_midge::MidgeError> {
        self.inner().scan(query)
    }

    pub(crate) fn set_conflict_policy(&mut self, policy: ConflictPolicy) {
        self.inner_mut().set_conflict_policy(policy);
    }

    pub(crate) fn assert_expected_value(
        &mut self,
        key: Vec<u8>,
        expected: &ExpectedStorageValue,
    ) -> Result<(), cntryl_midge::MidgeError> {
        match expected {
            ExpectedStorageValue::Untracked => Ok(()),
            ExpectedStorageValue::Missing => self.inner_mut().assert_value(key, None),
            ExpectedStorageValue::Present(value) => {
                self.inner_mut().assert_value(key, Some(value.clone()))
            }
        }
    }

    pub(crate) fn commit(mut self, options: WriteOptions) -> Result<(), cntryl_midge::MidgeError> {
        let inner = self.inner.take().expect("commit consumes its transaction");
        if self.suppress_commit {
            drop(inner);
            return Ok(());
        }
        if let Some(overlay) = self.overlay {
            if self.stage_writes {
                let mut shared = overlay.lock();
                for (key, mut change) in self.local_changes {
                    if let Some(existing) = shared.changes.get(&key) {
                        change.expected.clone_from(&existing.expected);
                    }
                    shared.changes.insert(key, change);
                }
            }
            drop(inner);
            return Ok(());
        }
        inner.commit(options)
    }

    pub(crate) fn rollback(mut self) -> Result<(), cntryl_midge::MidgeError> {
        let inner = self
            .inner
            .take()
            .expect("rollback consumes its transaction");
        if self.overlay.is_some() {
            drop(inner);
            Ok(())
        } else {
            inner.rollback()
        }
    }
}

impl std::ops::Deref for AdapterTransaction {
    type Target = cntryl_midge::Transaction;

    fn deref(&self) -> &Self::Target {
        self.inner()
    }
}

impl std::ops::DerefMut for AdapterTransaction {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner_mut()
    }
}

impl Midge {
    pub(crate) fn enter_transactional_ddl_scope(
        &self,
        database: &str,
        overlay: Arc<Mutex<StorageOverlay>>,
        stage_writes: bool,
    ) -> Result<StorageScopeGuard, CassieError> {
        let family = self.database_family(database)?;
        Ok(StorageScopeGuard::enter(ActiveStorageScope {
            midge_identity: std::ptr::from_ref(self) as usize,
            family_id: family.id(),
            overlay,
            stage_writes,
        }))
    }

    pub(crate) fn begin_adapter_tx(
        &self,
        family_id: cntryl_midge::ColumnFamilyId,
        mode: TransactionMode,
    ) -> Result<AdapterTransaction, CassieError> {
        let active = ACTIVE_STORAGE_SCOPE.with(|current| current.borrow().clone());
        let Some(scope) =
            active.filter(|scope| scope.midge_identity == std::ptr::from_ref(self) as usize)
        else {
            return self
                .engine
                .begin_tx(family_id, mode)
                .map(AdapterTransaction::raw)
                .map_err(CassieError::from);
        };
        let is_database_family = self
            .database_families
            .read()
            .values()
            .any(|family| family.handle.id() == family_id);
        if is_database_family && family_id != scope.family_id {
            return Err(CassieError::Unsupported(
                "transactional DDL cannot access multiple databases".to_string(),
            ));
        }
        if family_id != scope.family_id {
            return self
                .engine
                .begin_tx(family_id, mode)
                .map(AdapterTransaction::raw)
                .map_err(CassieError::from);
        }

        let overlay_changes = scope.overlay.lock().changes.clone();
        let seed_overlay = !overlay_changes.is_empty() && mode == TransactionMode::ReadOnly;
        if !scope.stage_writes && !seed_overlay {
            return self
                .engine
                .begin_tx(family_id, mode)
                .map(AdapterTransaction::raw)
                .map_err(CassieError::from);
        }

        let mut inner = self
            .engine
            .begin_tx(family_id, TransactionMode::ReadWrite)
            .map_err(CassieError::from)?;
        if scope.stage_writes {
            inner.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        }
        for (key, change) in overlay_changes {
            if let Some(value) = change.value {
                inner
                    .put(key, value, change.ttl_seconds)
                    .map_err(CassieError::from)?;
            } else {
                inner.delete(key).map_err(CassieError::from)?;
            }
        }
        Ok(AdapterTransaction::staged(
            inner,
            scope.overlay,
            scope.stage_writes,
            seed_overlay && !scope.stage_writes,
        ))
    }

    pub(crate) fn begin_database_adapter_tx(
        &self,
        database: &str,
        mode: TransactionMode,
    ) -> Result<AdapterTransaction, CassieError> {
        let family = self.database_family(database)?;
        self.begin_adapter_tx(family.id(), mode)
    }

    pub(crate) fn begin_raw_database_adapter_tx(
        &self,
        database: &str,
        mode: TransactionMode,
    ) -> Result<AdapterTransaction, CassieError> {
        let family = self.database_family(database)?;
        self.engine
            .begin_tx(family.id(), mode)
            .map(AdapterTransaction::raw)
            .map_err(CassieError::from)
    }

    pub(crate) fn commit_storage_overlay(
        &self,
        database: &str,
        overlay: &StorageOverlay,
    ) -> Result<(), CassieError> {
        if overlay.changes.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin_raw_database_adapter_tx(database, TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        for (key, change) in &overlay.changes {
            tx.assert_expected_value(key.clone(), &change.expected)
                .map_err(CassieError::from)?;
        }
        for (key, change) in &overlay.changes {
            if let Some(value) = &change.value {
                tx.put(key.clone(), value.clone(), change.ttl_seconds)
                    .map_err(CassieError::from)?;
            } else {
                tx.delete(key.clone()).map_err(CassieError::from)?;
            }
        }
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}

impl Midge {
    pub(super) fn database_names(&self) -> Vec<String> {
        self.database_families.read().keys().cloned().collect()
    }

    pub(super) fn scan_database_families(
        &self,
        prefix: &[u8],
    ) -> Result<Vec<RawStorageEntry>, CassieError> {
        let mut entries = Vec::new();
        for database in self.database_names() {
            let tx = self.begin_database_readonly_tx(&database)?;
            entries.extend(collect_scan(
                tx.scan(&Query::new().prefix(prefix.to_vec().into()))
                    .map_err(CassieError::from)?,
            )?);
        }
        Ok(entries)
    }

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
    /// column family. Global cf0 and temporary transactions remain separate and
    /// cannot be combined with this transaction.
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
    ) -> Result<AdapterTransaction, CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        self.begin_adapter_tx(family.id(), mode)
    }

    pub(super) fn database_tx_for_namespace(
        &self,
        namespace: &str,
        mode: TransactionMode,
    ) -> Result<AdapterTransaction, CassieError> {
        let canonical = self.canonical_namespace_name(namespace);
        let database =
            schema_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        self.begin_adapter_tx(family.id(), mode)
    }

    pub(super) fn begin_namespace_rw_tx(
        &self,
        namespace: &str,
    ) -> Result<AdapterTransaction, CassieError> {
        let canonical = self.canonical_namespace_name(namespace);
        let database =
            schema_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        let mut tx = self.begin_adapter_tx(family.id(), TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        Ok(tx)
    }

    pub(super) fn flush_data_family_for_collection(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        self.engine.flush_cf(&family).map_err(CassieError::from)
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

    pub(super) fn begin_schema_readonly_tx(&self) -> Result<AdapterTransaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        self.begin_adapter_tx(layout.schema.id(), TransactionMode::ReadOnly)
    }

    pub(super) fn begin_schema_rw_tx(&self) -> Result<AdapterTransaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        let mut tx = self.begin_adapter_tx(layout.schema.id(), TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        Ok(tx)
    }

    pub(super) fn begin_data_readonly_tx_for(
        &self,
        collection: &str,
    ) -> Result<super::DataReadTransaction, CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        self.begin_database_readonly_tx(&database)
    }

    pub(super) fn begin_database_readonly_tx(
        &self,
        database: &str,
    ) -> Result<super::DataReadTransaction, CassieError> {
        if let Some(tx) = self.statement_data_read(database) {
            return Ok(tx);
        }
        let family = self.database_family(database)?;
        self.begin_adapter_tx(family.id(), TransactionMode::ReadOnly)
            .map(super::DataReadTransaction::fresh)
    }

    pub(super) fn begin_family_readonly_tx(
        &self,
        family: StorageFamily,
    ) -> Result<super::DataReadTransaction, CassieError> {
        if family == StorageFamily::Data {
            return self.begin_database_readonly_tx(&self.default_database);
        }
        self.transaction(family, TransactionMode::ReadOnly)
            .map(AdapterTransaction::raw)
            .map(super::DataReadTransaction::fresh)
    }

    pub(super) fn begin_named_readonly_tx(
        &self,
        family: &str,
    ) -> Result<super::DataReadTransaction, CassieError> {
        let cf = self.engine.get_column_family(family).ok_or_else(|| {
            CassieError::StorageMissingFamily(format!(
                "required column family '{family}' is missing"
            ))
        })?;
        if let Some(tx) = self.statement_data_read_for_family(cf.id()) {
            return Ok(tx);
        }
        self.begin_adapter_tx(cf.id(), TransactionMode::ReadOnly)
            .map(super::DataReadTransaction::fresh)
    }

    pub(super) fn begin_data_rw_tx_for(
        &self,
        collection: &str,
    ) -> Result<AdapterTransaction, CassieError> {
        let canonical = self.canonical_collection_name(collection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        let mut tx = self.begin_adapter_tx(family.id(), TransactionMode::ReadWrite)?;
        tx.set_conflict_policy(ConflictPolicy::AbortOnWriteConflict);
        Ok(tx)
    }
}
