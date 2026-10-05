use std::mem::size_of;
use std::sync::Arc;

use crate::catalog::CollectionSchema;
use crate::executor::retained_memory::{add, grown_capacity, lookup_bytes, mul};
use crate::midge::adapter::DocumentRef;
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryMemoryReservation;

use super::super::accounting;
use super::{
    check_timeout, qualify_row, scan, BatchRow, JoinRetentionContext, JoinRetentionPhase,
    QueryError, SourceExecutionEnv,
};

#[cfg(test)]
#[path = "source_retention_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct SourceRowShape<'a> {
    pub(super) collection: &'a str,
    pub(super) fields: &'a [String],
    pub(super) schema: Option<&'a CollectionSchema>,
}

/// The source cursor retains its decoded document while this separate owned row is built.
pub(super) fn project_source_row(
    env: &SourceExecutionEnv<'_>,
    document: &DocumentRef,
    shape: SourceRowShape<'_>,
    phase: JoinRetentionPhase,
    retention: &JoinRetentionContext<'_>,
) -> Result<BatchRow, QueryError> {
    check_timeout(env.controls)?;
    retention.enter(phase);
    let projection = scan::ordered_projection_shape(
        document,
        shape
            .fields
            .iter()
            .map(|field| (field.as_str(), field.as_str())),
        shape.schema,
        None,
        env.controls,
    )?;
    let entries = add(shape.fields.len(), 1)?;
    let names = shape.fields.iter().try_fold(
        crate::types::row_identity::ROW_IDENTITY_COLUMN.len(),
        |bytes, field| add(bytes, field.len()),
    )?;
    let aliases = qualification_bytes(shape.collection, entries, names, 0, 0)?;
    let lease = size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>();
    let memory = env
        .controls
        .reserve_query_memory(add(projection.bytes, add(aliases, lease)?)?)?;
    let _qualification_scratch = qualification_scratch(env, shape.collection)?;
    retention.before(phase)?;
    let row = qualify_row(
        scan::projected_document_to_row(document, shape.fields, shape.schema),
        shape.collection,
    );
    check_timeout(env.controls)?;
    Ok(row.with_query_memory(Some(Arc::new(memory))))
}

/// Controlled full-row conversion owns the input body; this container owns new aliases/slots.
pub(super) fn load_collection_rows(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
) -> Result<AccountedVec<BatchRow>, QueryError> {
    let (batches, mut body_memory) =
        scan::scan_limit_retained(env.cassie, env.session, collection, None, env.controls)?;
    // Keep the converter's original body/metadata reservation through qualification and
    // build/probe use. Admit the shared lease itself before allocating its Arc header.
    body_memory.try_grow(size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>())?;
    let body_memory = Arc::new(body_memory);
    let mut rows = AccountedVec::try_new(env.controls)?;
    let _qualification_scratch = qualification_scratch(env, collection)?;
    for row in batches.into_iter().flatten() {
        let row = row.with_query_memory(Some(Arc::clone(&body_memory)));
        check_timeout(env.controls)?;
        let names = row
            .entries()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        let alias_names = row
            .aliases()
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        let aliases = qualification_bytes(
            collection,
            row.entries().len(),
            names,
            row.aliases().len(),
            alias_names,
        )?;
        rows.try_push_with(aliases, || qualify_row(row, collection))?;
    }
    drop(body_memory);
    check_timeout(env.controls)?;
    Ok(rows)
}

/// The key copy and any first lookup/parser scratch remain admitted through the caller's probe.
pub(super) fn reserve_probe_key(
    env: &SourceExecutionEnv<'_>,
    row: &BatchRow,
    column: &str,
) -> Result<QueryMemoryReservation, QueryError> {
    check_timeout(env.controls)?;
    let bytes = add(
        accounting::join_key_bytes(row, column)?,
        accounting::key_lookup_scratch_bytes(std::slice::from_ref(row), column)?,
    )?;
    Ok(env.controls.reserve_query_memory(bytes)?)
}

fn qualification_scratch(
    env: &SourceExecutionEnv<'_>,
    collection: &str,
) -> Result<QueryMemoryReservation, QueryError> {
    // Qualifier parsing builds component/suffix strings before BatchRow's eager lookup.
    Ok(env
        .controls
        .reserve_query_memory(add(512, mul(collection.len(), 64)?)?)?)
}

fn qualification_bytes(
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
