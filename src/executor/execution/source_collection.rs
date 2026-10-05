use std::mem::size_of;
use std::sync::Arc;

use crate::executor::retained_memory::{add, grown_capacity, lookup_bytes, mul};
use crate::runtime::QueryMemoryReservation;

use super::{check_timeout, qualify_row, scan, Batch, BatchRow, QueryError, SourceExecutionEnv};

/// Retain the full converter's original lease through qualification and source handoff.
pub(super) fn scan_collection(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
    row_budget: Option<usize>,
    qualify: bool,
    qualifier: &str,
) -> Result<Vec<Batch>, QueryError> {
    let (mut batches, mut memory) = scan::scan_limit_retained(
        env.cassie,
        env.session,
        collection,
        row_budget,
        env.controls,
    )?;
    check_timeout(env.controls)?;
    if batches.iter().all(Vec::is_empty) {
        return Ok(batches);
    }

    // The converter already owns all row bodies, type metadata, and batch slots. The
    // shared Arc and newly qualified aliases/lookups are additional retained state.
    memory.try_grow(size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>())?;
    if qualify {
        let aliases = batches.iter().flatten().try_fold(0, |bytes, row| {
            add(bytes, row_qualification_bytes(qualifier, row)?)
        })?;
        memory.try_grow(aliases)?;
        let _scratch = qualification_scratch(env, qualifier)?;
        for row in batches.iter_mut().flatten() {
            check_timeout(env.controls)?;
            // Move each body in place so qualification does not allocate another row
            // container while the converter's original batch buffers remain live.
            let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
            *row = qualify_row(owned, qualifier);
        }
    }
    check_timeout(env.controls)?;
    let memory = Arc::new(memory);
    for row in batches.iter_mut().flatten() {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&memory)));
    }
    check_timeout(env.controls)?;
    Ok(batches)
}

pub(super) fn row_qualification_bytes(
    collection: &str,
    row: &BatchRow,
) -> Result<usize, crate::app::CassieError> {
    let names = row
        .entries()
        .iter()
        .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
    let alias_names = row
        .aliases()
        .iter()
        .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
    qualification_bytes(
        collection,
        row.entries().len(),
        names,
        row.aliases().len(),
        alias_names,
    )
}

pub(super) fn qualification_scratch(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
) -> Result<QueryMemoryReservation, QueryError> {
    // Qualifier parsing builds component/suffix strings before BatchRow's eager lookup.
    Ok(env
        .controls
        .reserve_query_memory(add(512, mul(collection.len(), 64)?)?)?)
}

pub(super) fn qualification_bytes(
    collection: &str,
    entries: usize,
    names: usize,
    old_aliases: usize,
    old_alias_names: usize,
) -> Result<usize, crate::app::CassieError> {
    // Dots inside quoted components only increase this upper bound. Canonical qualifier
    // suffixes cannot exceed the conservatively escaped full relation name below.
    let variants = add(collection.bytes().filter(|byte| *byte == b'.').count(), 1)?;
    let maximum_qualifier = add(mul(collection.len(), 2)?, mul(variants, 2)?)?;
    let additional_aliases = mul(entries, variants)?;
    let alias_names = mul(
        variants,
        add(names, mul(entries, add(maximum_qualifier, 1)?)?)?,
    )?;
    let aliases = add(old_aliases, additional_aliases)?;
    let capacity = grown_capacity(aliases, 4)?;
    let buffers = mul(capacity, size_of::<(String, usize)>())?;
    let lookup = lookup_bytes(
        add(entries, aliases)?,
        add(names, add(old_alias_names, alias_names)?)?,
    )?;
    add(buffers, add(alias_names, lookup)?)
}
