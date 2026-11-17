//! A fork owns its context payload, database bytes and descriptor lease aliases.

use super::SessionSchemaContext;
use crate::app::{Cassie, CassieError};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard};
use std::sync::Arc;

#[derive(Debug)]
pub(in crate::app) struct OwnedSessionSchemaContext {
    state: Mutex<ContextState>,
}

#[derive(Debug)]
struct ContextState {
    context: Option<SessionSchemaContext>,
    memory: Option<QueryMemoryReservation>,
}

impl OwnedSessionSchemaContext {
    pub(in crate::app) fn empty() -> Self {
        Self {
            state: Mutex::new(ContextState {
                context: None,
                memory: None,
            }),
        }
    }

    pub(in crate::app) fn lock(&self) -> MappedMutexGuard<'_, Option<SessionSchemaContext>> {
        MutexGuard::map(self.state.lock(), |state| &mut state.context)
    }

    pub(in crate::app) fn initial(app: &Cassie) -> Self {
        Self {
            state: Mutex::new(ContextState {
                context: Some(from_app(app, app.default_database.clone())),
                memory: None,
            }),
        }
    }

    pub(in crate::app) fn fork_with_controls(
        &self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Arc<Self>, CassieError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let state = self.state.lock();
        let database_bytes = state
            .context
            .as_ref()
            .map_or(0, |context| context.default_database.len());
        let bytes = base_bytes()
            .checked_add(database_bytes)
            .ok_or_else(size_error)?;
        let memory = controls
            .map(|controls| controls.reserve_query_memory(bytes))
            .transpose()?;
        let context = state
            .context
            .as_ref()
            .map(|context| {
                Ok::<_, CassieError>(SessionSchemaContext {
                    catalog: context.catalog.clone(),
                    write_catalog: context.write_catalog.clone(),
                    publication_catalog: context.publication_catalog.clone(),
                    owner: context.owner.clone(),
                    default_database: copy_database(&context.default_database)?,
                    midge: context.midge.clone(),
                })
            })
            .transpose()?;
        Ok(Arc::new(Self {
            state: Mutex::new(ContextState { context, memory }),
        }))
    }

    pub(in crate::app) fn replace_from(&self, app: &Cassie) -> Result<(), CassieError> {
        let mut state = self.state.lock();
        if let Some(context) = state
            .context
            .as_mut()
            .filter(|context| context.default_database == app.default_database)
        {
            context.catalog = app.catalog.clone();
            context.write_catalog = app.metadata_write_catalog.clone();
            context.publication_catalog = app.publication_catalog.clone();
            context.owner.clone_from(&app.catalog_owner);
            context.midge = Arc::downgrade(&app.midge);
            return Ok(());
        }
        // Keep the old String charged until its replacement is allocated and
        // installed. The temporary admission then joins this same Arc owner.
        let temporary = state
            .memory
            .as_ref()
            .map(|memory| memory.try_sibling(app.default_database.len()))
            .transpose()?;
        let database = copy_database(&app.default_database)?;
        let old = state.context.replace(from_app(app, database));
        drop(old);
        if let (Some(memory), Some(temporary)) = (state.memory.as_mut(), temporary) {
            memory.shrink_to(base_bytes());
            memory.absorb(temporary);
        }
        Ok(())
    }
}

fn base_bytes() -> usize {
    std::mem::size_of::<OwnedSessionSchemaContext>() + 2 * std::mem::size_of::<usize>()
}

fn size_error() -> CassieError {
    CassieError::ResourceLimit("schema context size overflow".into())
}

fn copy_database(database: &str) -> Result<String, CassieError> {
    let mut copy = String::new();
    copy.try_reserve_exact(database.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if copy.capacity() > database.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted schema database capacity".into(),
        ));
    }
    copy.push_str(database);
    Ok(copy)
}

fn from_app(app: &Cassie, database: String) -> SessionSchemaContext {
    SessionSchemaContext {
        catalog: app.catalog.clone(),
        write_catalog: app.metadata_write_catalog.clone(),
        publication_catalog: app.publication_catalog.clone(),
        owner: app.catalog_owner.clone(),
        default_database: database,
        midge: Arc::downgrade(&app.midge),
    }
}
