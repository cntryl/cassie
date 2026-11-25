//! Owned pending and indexed row copies for the lazy graph read path.

use crate::midge::fulltext::FulltextRequestReads;

use std::collections::HashSet;
use std::mem::size_of;
use std::sync::Arc;

use super::access::KeyIndex;
use super::cache::{reserve_map, with_cache, Pending};
use super::keys::{key_of, KeyPart};
use super::store::Reads;
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::executor::retained_memory::{add, hash_table_bytes, mul, value_clone_bytes};
use crate::midge::adapter::{AccountedDocument, DocumentRef, ScalarIndexScanRequest};
use crate::runtime::owned_memory::{allocation_error, grow_vec};
use crate::runtime::QueryMemoryReservation;
use crate::types::DataType;

pub(super) fn row_copy(
    row: &BatchRow,
    memory: &mut QueryMemoryReservation,
) -> Result<BatchRow, QueryError> {
    memory.try_grow(add(size_of::<BatchRow>() + 64, row.owned_body_bytes()?)?)?;
    Ok(row
        .clone()
        .with_query_memory(None)
        .with_operator_memory(None))
}

/// Rehome moved source buffers under a narrow owner; a cached row must not retain
/// `OperatorMemory`'s controls back into the statement cache that contains it.
pub(super) fn detach_row(
    row: BatchRow,
    memory: &mut QueryMemoryReservation,
) -> Result<BatchRow, QueryError> {
    memory.try_grow(add(size_of::<BatchRow>() + 64, row.owned_body_bytes()?)?)?;
    let types = row.shared_data_types();
    let aliases = row.aliases().to_vec();
    Ok(BatchRow::with_aliases(row.into_entries(), aliases).with_optional_data_types(types))
}

fn hydrate(
    env: &SourceExecutionEnv<'_>,
    document: &DocumentRef,
    schema: Option<&crate::catalog::CollectionSchema>,
    memory: &mut QueryMemoryReservation,
) -> Result<BatchRow, QueryError> {
    memory.try_grow(add(
        64,
        crate::executor::scan::full_row_bytes(document, schema)?,
    )?)?;
    #[cfg(test)]
    crate::executor::execution::graph_memory_test_observer::record(
        "copy_lookup_row",
        env.controls.current_query_memory_bytes(),
    );
    let row = crate::executor::scan::hydrate_character_arrays(
        crate::executor::scan::document_to_row(document, schema),
        env.controls,
    )?;
    // OperatorMemory owns controls, whose statement state owns this graph cache.
    // Detach a hydrated copy under this narrow owner to avoid retaining that cycle.
    if row.operator_memory().is_some() {
        detach_row(row, memory)
    } else {
        Ok(row)
    }
}

fn matches_key(
    env: &SourceExecutionEnv<'_>,
    row: &BatchRow,
    columns: &[String],
    types: &[DataType],
    key: &[KeyPart],
) -> Result<bool, QueryError> {
    let bytes = columns.iter().try_fold(
        mul(columns.len(), size_of::<KeyPart>())?,
        |bytes, column| add(bytes, row.get(column).map_or(Ok(0), value_clone_bytes)?),
    )?;
    let _memory = env.controls.reserve_query_memory(bytes)?;
    Ok(key_of(|column| row.get(column), columns, types).as_deref() == Some(key))
}

/// The cache retains both the pending row bodies and their identity/directory owner.
fn pending_of(
    env: &SourceExecutionEnv<'_>,
    reads: &mut Reads,
    table: &str,
) -> Result<Arc<Pending>, QueryError> {
    if let Some(known) = with_cache(env, |cache| Ok(cache.pending.get(table).cloned()))? {
        return Ok(known);
    }
    let mut pending = Pending {
        changes: 0,
        ids: HashSet::new(),
        rows: Vec::new(),
        memory: env
            .controls
            .reserve_query_memory(size_of::<Pending>() + 2 * size_of::<usize>())?,
    };
    if let Some(session) = env.session {
        let snapshot = session.staged_write_snapshot(table);
        let count = snapshot.ordered_changes().len();
        pending
            .memory
            .try_grow(hash_table_bytes::<String>(count)?)?;
        pending
            .ids
            .try_reserve(count)
            .map_err(|error| allocation_error(&error))?;
        let schema = env.cassie.catalog.get_schema(table);
        for (id, change) in snapshot.ordered_changes() {
            env.controls.check_interruption()?;
            pending.changes += 1;
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "before_pending",
                env.controls.current_query_memory_bytes(),
            );
            pending.memory.try_grow(add(
                mul(id.len(), 2)?,
                size_of::<BatchRow>() + 2 * size_of::<String>(),
            )?)?;
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "copy_pending",
                env.controls.current_query_memory_bytes(),
            );
            pending.ids.insert(id.clone());
            if let crate::app::TransactionRowChange::Upsert(payload) = change {
                let bytes = add(
                    size_of::<AccountedDocument>(),
                    add(
                        id.len(),
                        crate::runtime::accounted::json::retained_bytes(payload)?,
                    )?,
                )?;
                let document =
                    AccountedDocument::try_build(&env.controls.storage_request(), bytes, || {
                        Ok(DocumentRef {
                            id: id.clone(),
                            payload: payload.clone(),
                        })
                    })?;
                let row = hydrate(
                    env,
                    document.document(),
                    schema.as_ref(),
                    &mut pending.memory,
                )?;
                grow_vec(&mut pending.rows, 1, &mut pending.memory)?;
                pending.rows.push((id.clone(), row));
            }
        }
    }
    reads.overlay_changes += pending.changes as u64;
    let pending = Arc::new(pending);
    with_cache(env, |cache| {
        reserve_map(&mut cache.pending, &mut cache.memory)?;
        cache.admit(table.len())?;
        cache.pending.insert(table.to_owned(), Arc::clone(&pending));
        Ok(())
    })?;
    Ok(pending)
}

/// Indexed hits and matching pending rows keep admitted bodies and directory backing.
pub(super) fn lookup_rows(
    env: &SourceExecutionEnv<'_>,
    reads: &mut Reads,
    table: &str,
    index: &KeyIndex,
    (columns, types): (&[String], &[DataType]),
    key: &[KeyPart],
) -> Result<Vec<BatchRow>, QueryError> {
    env.controls.check_interruption()?;
    let pending = pending_of(env, reads, table)?;
    let key_heap = key
        .iter()
        .try_fold(0, |bytes, part| add(bytes, part.heap_bytes()))?;
    let _request_memory = env.controls.reserve_query_memory(add(
        key_heap,
        mul(key.len(), size_of::<serde_json::Value>())?,
    )?)?;
    let request = ScalarIndexScanRequest {
        equality_prefix: index.probe(key),
        ..ScalarIndexScanRequest::default()
    };
    let hits = env.cassie.midge.scan_scalar_index_controlled(
        &index.index,
        &request,
        &env.controls.storage_request(),
    )?;
    let (hits, _hits_memory) = hits.into_parts();
    reads.index_entries += hits.len() as u64;
    let schema = env.cassie.catalog.get_schema(table);
    let mut directory = env.controls.reserve_query_memory(64)?;
    let mut rows = Vec::new();
    for hit in hits {
        env.controls.check_interruption()?;
        if pending.ids.contains(&hit.id) {
            continue;
        }
        reads.row_fetches += 1;
        let Some(source) =
            env.cassie
                .midge
                .get_retrieval_document_controlled(table, &hit.id, env.controls)?
        else {
            continue;
        };
        let (document, _source_memory) = source.into_parts();
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "before_lookup_row",
            env.controls.current_query_memory_bytes(),
        );
        let row = hydrate(env, &document, schema.as_ref(), &mut directory)?;
        if matches_key(env, &row, columns, types, key)? {
            grow_vec(&mut rows, 1, &mut directory)?;
            rows.push(row);
        }
    }
    for (_, row) in &pending.rows {
        env.controls.check_interruption()?;
        if matches_key(env, row, columns, types, key)? {
            grow_vec(&mut rows, 1, &mut directory)?;
            rows.push(row_copy(row, &mut directory)?);
        }
    }
    let directory = Arc::new(directory);
    for row in &mut rows {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&directory)));
    }
    Ok(rows)
}

#[cfg(test)]
mod memory_tests {
    use crate::executor::execution::graph_memory_test_observer as observer;

    const QUERY: &str =
        "SELECT * FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
        COLUMNS(b.id)) AS t";

    #[test]
    fn should_admit_a_lazy_lookup_row_before_hydrating_its_copy() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(&session, QUERY, vec![])
            .expect("indexed graph query");

        // Assert
        assert_eq!(result.row_count(), 1);
        observer::assert_admitted(
            "before_lookup_row",
            "copy_lookup_row",
            std::mem::size_of::<super::BatchRow>(),
        );
    }

    #[test]
    fn should_admit_pending_row_identity_before_cloning_it_into_the_graph_cache() {
        // Arrange
        let (cassie, session) = observer::fixture();
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin overlay");
        cassie
            .execute_sql(&session, "INSERT INTO e VALUES(2,1,2)", vec![])
            .expect("pending edge");
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(&session, QUERY, vec![])
            .expect("merged graph query");

        // Assert
        assert_eq!(result.row_count(), 2);
        observer::assert_admitted(
            "before_pending",
            "copy_pending",
            std::mem::size_of::<String>() + std::mem::size_of::<super::BatchRow>(),
        );
    }
}
