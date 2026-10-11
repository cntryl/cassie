//! Scoped SQL result-cache inputs; compiled-plan input ownership is a separate boundary.

use std::mem::size_of;

use crate::catalog::DEFAULT_SCHEMA;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::owned_memory::allocation_error;
use crate::runtime::{
    ExecutionMode, ExecutionResultCacheKey, QueryExecutionControls, QueryMemoryReservation,
};
use crate::types::Value;

use super::{Cassie, CassieError, CassieSession};

impl Cassie {
    pub(super) fn admitted_execution_result_cache_key(
        &self,
        session: &CassieSession,
        sql_fingerprint: u64,
        params: &[Value],
        mode: ExecutionMode,
        controls: &QueryExecutionControls,
    ) -> Result<(ExecutionResultCacheKey, QueryMemoryReservation), CassieError> {
        controls.check_interruption()?;
        let params_hash = crate::runtime::hash_params(params);
        let settings_fingerprint = session.settings_fingerprint();
        session.with_search_path(|path| {
            let bytes = key_bytes(session, path, controls)?;
            controls.check_interruption()?;
            let memory = controls.reserve_query_memory(bytes)?;
            let user = copy_text(&session.user, controls)?;
            let database = session.database.as_deref()
                .map(|database| copy_text(database, controls)).transpose()?;
            let search_path = copy_path(path, controls)?;
            let key = ExecutionResultCacheKey {
                sql_fingerprint,
                params_hash,
                schema_epoch: self.runtime.schema_epoch(),
                data_epoch: self.runtime.data_epoch(),
                user,
                database,
                search_path,
                settings_fingerprint,
                mode,
            };
            controls.check_interruption()?;
            Ok((key, memory))
        })
    }
}

fn key_bytes(
    session: &CassieSession,
    path: &[String],
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    let directory_length = path.len().max(1);
    let mut bytes = add(size_of::<ExecutionResultCacheKey>(), session.user.len())?;
    bytes = add(bytes, session.database.as_deref().map_or(0, str::len))?;
    bytes = add(bytes, mul(directory_length, size_of::<String>())?)?;
    if path.is_empty() {
        add(bytes, DEFAULT_SCHEMA.len())
    } else {
        path.iter().try_fold(bytes, |bytes, item| {
            controls.check_interruption()?;
            add(bytes, item.len())
        })
    }
}

fn copy_path(
    path: &[String],
    controls: &QueryExecutionControls,
) -> Result<Vec<String>, CassieError> {
    controls.check_interruption()?;
    let length = path.len().max(1);
    let mut copy = Vec::new();
    copy.try_reserve_exact(length).map_err(allocation_error)?;
    if copy.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted result-cache path capacity".to_owned(),
        ));
    }
    if path.is_empty() {
        copy.push(copy_text(DEFAULT_SCHEMA, controls)?);
    } else {
        for item in path {
            copy.push(copy_text(item, controls)?);
        }
    }
    Ok(copy)
}

fn copy_text(text: &str, controls: &QueryExecutionControls) -> Result<String, CassieError> {
    controls.check_interruption()?;
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).map_err(allocation_error)?;
    if copy.capacity() > text.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted result-cache text capacity".to_owned(),
        ));
    }
    let mut offset = 0;
    while offset < text.len() {
        controls.check_interruption()?;
        let mut end = offset.saturating_add(4096).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        copy.push_str(&text[offset..end]);
        offset = end;
    }
    Ok(copy)
}
