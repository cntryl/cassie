//! Admitted COPY descriptor and storage authority across the wire stream.

use super::{
    Arc, CassieError, CassieSession, Catalog, Mutex, SessionTransactionState,
    SessionTransactionStatus, StatementMutationBatch,
};
use crate::app::Cassie;
use crate::catalog::ControlledCatalogSnapshot;
use crate::midge::adapter::CopyStorageSnapshot;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::sync::Weak;
use std::time::Duration;

pub(crate) struct CopyCommandOwner {
    identity: Weak<Mutex<SessionTransactionState>>,
    generation: u64,
    database: String,
    default_database: String,
    target: Option<(String, u64, u64)>,
    midge: Arc<crate::midge::adapter::Midge>,
    catalog: Arc<ControlledCatalogSnapshot>,
    storage: Arc<CopyStorageSnapshot>,
    publication_catalog: Catalog,
    publication_guard: Arc<parking_lot::RwLock<()>>,
    _memory: QueryMemoryReservation,
    _database_memory: QueryMemoryReservation,
    _default_database_memory: QueryMemoryReservation,
    target_memory: Option<QueryMemoryReservation>,
}

pub(crate) struct CopyBindingContext {
    context: crate::sql::binder::BindingContext,
    _memory: QueryMemoryReservation,
}

impl CopyBindingContext {
    pub(crate) fn context(&self) -> &crate::sql::binder::BindingContext {
        &self.context
    }
}

impl CopyCommandOwner {
    pub(crate) fn lock_catalog<'a>(
        &'a self,
        controls: &QueryExecutionControls,
    ) -> Result<parking_lot::RwLockReadGuard<'a, ()>, CassieError> {
        lock_publication(&self.publication_guard, controls)
    }

    pub(crate) fn catalog(&self) -> &Catalog {
        self.catalog.catalog()
    }
    pub(crate) fn catalog_owner(&self) -> Arc<ControlledCatalogSnapshot> {
        Arc::clone(&self.catalog)
    }

    fn bind_target_guarded(
        &mut self,
        session: &CassieSession,
        target: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let id = self
            .catalog
            .catalog()
            .collection_storage_id(target)
            .ok_or_else(|| CassieError::CollectionNotFound(target.to_owned()))?;
        let (name, memory) = copy_string(target, controls)?;
        let mut transaction = session.transaction.lock();
        if !std::ptr::eq(self.identity.as_ptr(), Arc::as_ptr(&session.transaction))
            || transaction.status != SessionTransactionStatus::InTransaction
            || transaction.generation != self.generation
            || session.current_database().unwrap_or(&self.default_database) != self.database
        {
            return Err(stale_copy());
        }
        if transaction.isolation == Some(crate::sql::ast::TransactionIsolation::RepeatableRead)
            && self
                .publication_catalog
                .collection_storage_id(target)
                .is_some_and(|published_id| published_id != id)
        {
            return Err(CassieError::Unsupported(
                "COPY into a recreated published target is not supported under REPEATABLE READ"
                    .into(),
            ));
        }
        let descriptor = CassieSession::command_descriptor_snapshot_locked(
            &mut transaction,
            &self.publication_catalog,
            controls,
        )?;
        let current = descriptor.as_ref().map_or_else(
            || {
                transaction
                    .catalog_overlay
                    .as_ref()
                    .unwrap_or(&self.publication_catalog)
            },
            |owner| owner.catalog(),
        );
        let overlay = transaction.storage_overlay.lock();
        if !self
            .catalog
            .catalog()
            .matches_transaction_metadata(current, controls)?
            || !self.storage.matches(&overlay, controls)?
        {
            return Err(stale_copy());
        }
        let physical_id = self
            .midge
            .copy_physical_identity(&name, &overlay, controls)?
            .ok_or_else(|| {
                CassieError::Unsupported("COPY target has no stable physical identity".into())
            })?;
        drop(overlay);
        drop(transaction);
        self.target = Some((name, id, physical_id));
        self.target_memory = Some(memory);
        Ok(())
    }

    pub(crate) fn matches_target(&self, target: &str) -> bool {
        self.target
            .as_ref()
            .is_some_and(|(name, _, _)| name == target)
    }
}

fn copy_string(
    value: &str,
    controls: &QueryExecutionControls,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    let memory = controls.reserve_query_memory(value.len())?;
    let mut text = String::new();
    text.try_reserve_exact(value.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > value.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY identifier capacity".into(),
        ));
    }
    let mut start = 0;
    while start < value.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        text.push_str(&value[start..end]);
        start = end;
    }
    Ok((text, memory))
}

fn copy_search_path(
    session: &CassieSession,
    controls: &QueryExecutionControls,
) -> Result<(Vec<String>, QueryMemoryReservation), CassieError> {
    session.with_search_path(|path| {
        let length = path.len().max(1);
        let base = controls.reserve_query_memory(0)?;
        let mut memory;
        let mut copy;
        (copy, memory) = crate::runtime::owned_memory::try_reserve_vec(length, &base)?;
        for index in 0..length {
            controls.check_interruption()?;
            let value = path
                .get(index)
                .map_or(crate::catalog::DEFAULT_SCHEMA, String::as_str);
            let (text, lease) = copy_string(value, controls)?;
            copy.push(text);
            memory.absorb(lease);
        }
        Ok((copy, memory))
    })
}

fn copy_canonical_schema(
    database: &str,
    schema: &str,
    controls: &QueryExecutionControls,
) -> Result<(String, QueryMemoryReservation), CassieError> {
    let component = |name: &str| -> Result<(usize, bool), CassieError> {
        let mut quoted = false;
        let mut quote_count = 0_usize;
        for (offset, character) in name.char_indices() {
            if offset % 4096 < character.len_utf8() {
                controls.check_interruption()?;
            }
            quoted |= character == '.' || character == '"' || character.is_whitespace();
            quote_count += usize::from(character == '"');
        }
        let length = name
            .len()
            .checked_add(quote_count)
            .and_then(|value| value.checked_add(if quoted { 2 } else { 0 }))
            .ok_or_else(|| CassieError::ResourceLimit("COPY schema capacity overflow".into()))?;
        Ok((length, quoted))
    };
    let (database_len, database_quoted) = component(database)?;
    let (schema_len, schema_quoted) = component(schema)?;
    let length = database_len
        .checked_add(schema_len)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| CassieError::ResourceLimit("COPY schema capacity overflow".into()))?;
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY schema capacity".into(),
        ));
    }
    for (index, (name, quoted)) in [(database, database_quoted), (schema, schema_quoted)]
        .into_iter()
        .enumerate()
    {
        if index != 0 {
            text.push('.');
        }
        if quoted {
            text.push('"');
        }
        for (offset, character) in name.char_indices() {
            if offset % 4096 < character.len_utf8() {
                controls.check_interruption()?;
            }
            text.push(character);
            if character == '"' {
                text.push('"');
            }
        }
        if quoted {
            text.push('"');
        }
    }
    Ok((text, memory))
}

fn copy_binding_context(
    catalog: &Catalog,
    database: &str,
    user: &str,
    raw_path: &[String],
    scoped: bool,
    controls: &QueryExecutionControls,
) -> Result<(crate::sql::binder::BindingContext, QueryMemoryReservation), CassieError> {
    let base = controls.reserve_query_memory(0)?;
    let mut memory;
    let mut search_path;
    (search_path, memory) =
        crate::runtime::owned_memory::try_reserve_vec(raw_path.len().max(1), &base)?;
    for entry in raw_path {
        controls.check_interruption()?;
        let schema = if entry == super::super::USER_SEARCH_PATH_ENTRY {
            user
        } else {
            entry
        };
        let _scratch;
        let canonical;
        (canonical, _scratch) = copy_canonical_schema(database, schema, controls)?;
        if !(schema.eq_ignore_ascii_case("pg_catalog")
            || schema.eq_ignore_ascii_case("information_schema")
            || schema.eq_ignore_ascii_case(crate::catalog::DEFAULT_SCHEMA)
            || catalog.namespace_exists_controlled(&canonical, controls)?)
        {
            continue;
        }
        let schema = crate::catalog::trim_catalog_name_controlled(schema, controls)?;
        if schema.is_empty() {
            continue;
        }
        let (schema, lease) = copy_string(schema, controls)?;
        search_path.push(schema);
        memory.absorb(lease);
    }
    if search_path.is_empty() {
        let (schema, lease) = copy_string(crate::catalog::DEFAULT_SCHEMA, controls)?;
        search_path.push(schema);
        memory.absorb(lease);
    }
    let (database, lease) = copy_string(database, controls)?;
    memory.absorb(lease);
    Ok((
        crate::sql::binder::BindingContext {
            database,
            search_path,
            enforce_database_scope: scoped,
        },
        memory,
    ))
}

fn lock_publication<'a>(
    guard: &'a parking_lot::RwLock<()>,
    controls: &QueryExecutionControls,
) -> Result<parking_lot::RwLockReadGuard<'a, ()>, CassieError> {
    loop {
        controls.check_interruption()?;
        if let Some(read) = guard.try_read_for(Duration::from_millis(2)) {
            controls.check_interruption()?;
            return Ok(read);
        }
    }
}

impl Cassie {
    pub(crate) fn capture_copy_command_view(
        &self,
        session: &CassieSession,
        controls: &QueryExecutionControls,
    ) -> Result<CopyCommandOwner, CassieError> {
        let _publication = lock_publication(&self.catalog_publication, controls)?;
        self.capture_copy_command_view_guarded(session, controls)
    }

    fn capture_copy_command_view_guarded(
        &self,
        session: &CassieSession,
        controls: &QueryExecutionControls,
    ) -> Result<CopyCommandOwner, CassieError> {
        let mut transaction = session.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction {
            return Err(CassieError::Execution(
                "COPY owner requires an active transaction".into(),
            ));
        }
        let descriptor = CassieSession::command_descriptor_snapshot_locked(
            &mut transaction,
            &self.publication_catalog,
            controls,
        )?;
        let catalog = if let Some(descriptor) = descriptor {
            descriptor
        } else {
            transaction
                .catalog_overlay
                .as_ref()
                .unwrap_or(&self.publication_catalog)
                .snapshot_with_controls(controls)?
        };
        let storage = transaction.storage_overlay.lock().copy_snapshot(controls)?;
        let database_source = session.current_database().unwrap_or(&self.default_database);
        let database_memory;
        let database;
        (database, database_memory) = copy_string(database_source, controls)?;
        let default_database_memory;
        let default_database;
        (default_database, default_database_memory) =
            copy_string(&self.default_database, controls)?;
        let memory = controls.reserve_query_memory(
            std::mem::size_of::<CopyCommandOwner>() + 2 * std::mem::size_of::<usize>(),
        )?;
        controls.check_interruption()?;
        Ok(CopyCommandOwner {
            identity: Arc::downgrade(&session.transaction),
            generation: transaction.generation,
            database,
            default_database,
            target: None,
            midge: Arc::clone(&self.midge),
            catalog,
            storage,
            publication_catalog: self.publication_catalog.clone(),
            publication_guard: Arc::clone(&self.catalog_publication),
            _memory: memory,
            _database_memory: database_memory,
            _default_database_memory: default_database_memory,
            target_memory: None,
        })
    }

    pub(crate) fn with_copy_command_owner(
        &self,
        session: &CassieSession,
        owner: &CopyCommandOwner,
    ) -> Self {
        let mut scoped = self.clone();
        scoped.catalog = owner.catalog().clone();
        scoped.catalog_owner = Some(owner.catalog_owner());
        scoped.metadata_write_catalog = session
            .transaction_catalog_overlay()
            .unwrap_or_else(|| self.publication_catalog.clone());
        scoped
    }

    pub(crate) fn capture_bound_copy_command_owner(
        &self,
        session: &CassieSession,
        target: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(CopyCommandOwner, CopyBindingContext), CassieError> {
        self.capture_bound_copy_command_owner_observed(session, target, controls, || Ok(()))
    }

    fn capture_bound_copy_command_owner_observed(
        &self,
        session: &CassieSession,
        target: &str,
        controls: &QueryExecutionControls,
        after_provisional: impl FnOnce() -> Result<(), CassieError>,
    ) -> Result<(CopyCommandOwner, CopyBindingContext), CassieError> {
        let (path, _path_memory) = copy_search_path(session, controls)?;
        let database = session.current_database().unwrap_or(&self.default_database);
        let provisional = self.capture_copy_command_view(session, controls)?;
        let context_memory;
        let context;
        (context, context_memory) = copy_binding_context(
            provisional.catalog(),
            database,
            &session.user,
            &path,
            self.database_catalog_enforced(),
            controls,
        )?;
        let candidate =
            crate::sql::binder::resolve_relation_name(target, provisional.catalog(), &context)?;
        drop(context);
        drop(context_memory);
        after_provisional()?;
        let _publication = provisional.lock_catalog(controls)?;
        self.midge.with_transaction_write_gates(
            std::slice::from_ref(&candidate),
            true,
            controls,
            || {
                let mut owner = self.capture_copy_command_view_guarded(session, controls)?;
                let context_memory;
                let context;
                (context, context_memory) = copy_binding_context(
                    owner.catalog(),
                    database,
                    &session.user,
                    &path,
                    self.database_catalog_enforced(),
                    controls,
                )?;
                let resolved =
                    crate::sql::binder::resolve_relation_name(target, owner.catalog(), &context)?;
                if resolved != candidate {
                    return Err(stale_copy());
                }
                owner.bind_target_guarded(session, &resolved, controls)?;
                Ok((
                    owner,
                    CopyBindingContext {
                        context,
                        _memory: context_memory,
                    },
                ))
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn capture_copy_command_owner(
        &self,
        session: &CassieSession,
        target: &str,
        controls: &QueryExecutionControls,
    ) -> Result<CopyCommandOwner, CassieError> {
        self.capture_bound_copy_command_owner(session, target, controls)
            .map(|(owner, _)| owner)
    }
}

impl CassieSession {
    #[cfg(test)]
    pub(crate) fn fork_copy_statement_batch(
        &self,
        owner: &CopyCommandOwner,
        controls: &QueryExecutionControls,
    ) -> Result<StatementMutationBatch, CassieError> {
        let publication = owner.lock_catalog(controls)?;
        let (target, _, _) = owner.target.as_ref().ok_or_else(stale_copy)?;
        owner.midge.with_transaction_write_gates(
            std::slice::from_ref(target),
            true,
            controls,
            || self.fork_copy_statement_batch_guarded(owner, controls, &publication),
        )
    }

    pub(crate) fn fork_copy_statement_batch_guarded(
        &self,
        owner: &CopyCommandOwner,
        controls: &QueryExecutionControls,
        _publication: &parking_lot::RwLockReadGuard<'_, ()>,
    ) -> Result<StatementMutationBatch, CassieError> {
        let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(controls));
        self.fork_statement_batch_checked(|transaction| {
            controls.check_interruption()?;
            if !std::ptr::eq(owner.identity.as_ptr(), Arc::as_ptr(&self.transaction))
                || transaction.status != SessionTransactionStatus::InTransaction
                || transaction.generation != owner.generation
                || self.current_database().unwrap_or(&owner.default_database) != owner.database
            {
                return Err(stale_copy());
            }
            let descriptor = Self::command_descriptor_snapshot_locked(
                transaction,
                &owner.publication_catalog,
                controls,
            )?;
            let current = descriptor.as_ref().map_or_else(
                || {
                    transaction
                        .catalog_overlay
                        .as_ref()
                        .unwrap_or(&owner.publication_catalog)
                },
                |owner| owner.catalog(),
            );
            let Some((target, id, physical_id)) = &owner.target else {
                return Err(stale_copy());
            };
            let overlay = transaction.storage_overlay.lock();
            if current.collection_storage_id(target) != Some(*id)
                || !owner
                    .catalog
                    .catalog()
                    .matches_transaction_metadata(current, controls)?
                || !owner.storage.matches(&overlay, controls)?
                || owner
                    .midge
                    .copy_physical_identity(target, &overlay, controls)?
                    != Some(*physical_id)
            {
                return Err(stale_copy());
            }
            drop(overlay);
            controls.check_interruption()?;
            Ok(())
        })
    }
}

fn stale_copy() -> CassieError {
    CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message: "COPY owner no longer matches the active transaction".to_owned(),
    }
}

#[cfg(test)]
#[path = "copy_owner_tests.rs"]
mod tests;
