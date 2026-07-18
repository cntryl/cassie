use std::collections::{BTreeMap, BTreeSet};

use cntryl_midge::Query;

use super::{
    codec, validate_manifest_header, FulltextManifest, LoadedCandidatePostings,
    PersistedFulltextCandidateSet, PersistedFulltextDocumentStats, STATE_VERSION,
};
use crate::app::CassieError;
use crate::midge::adapter::{DocumentRef, Midge};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;

pub(crate) struct ControlledFulltextCandidateSet {
    candidates: PersistedFulltextCandidateSet,
    memory: QueryMemoryReservation,
}

pub(crate) struct ControlledRetrievalDocument {
    document: DocumentRef,
    memory: QueryMemoryReservation,
}

impl ControlledRetrievalDocument {
    pub(crate) fn into_parts(self) -> (DocumentRef, QueryMemoryReservation) {
        (self.document, self.memory)
    }
}

const BTREE_ENTRY_OVERHEAD: usize = 4 * std::mem::size_of::<usize>();

fn retained_string_bytes(value: &str) -> usize {
    value
        .len()
        .saturating_add(std::mem::size_of::<String>())
        .saturating_add(BTREE_ENTRY_OVERHEAD)
}

fn checked_accounting_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(|| {
        CassieError::ResourceLimit("controlled fulltext retrieval accounting overflow".to_owned())
    })
}

fn checked_accounting_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(|| {
        CassieError::ResourceLimit("controlled fulltext retrieval accounting overflow".to_owned())
    })
}

const fn column_decode_expansion_factor(data_type: &DataType) -> usize {
    match data_type {
        DataType::Array(_) => 40,
        DataType::Json => 24,
        DataType::Vector(_) => 10,
        DataType::Bytea => 3,
        _ => 2,
    }
}

fn provisional_column_field_bytes(
    data_type: &DataType,
    field_name: &str,
    raw_key_bytes: usize,
    raw_value_bytes: usize,
) -> Result<usize, CassieError> {
    let decoded_bytes =
        checked_accounting_mul(raw_value_bytes, column_decode_expansion_factor(data_type))?;
    [
        raw_key_bytes,
        raw_value_bytes,
        retained_string_bytes(field_name),
        std::mem::size_of::<serde_json::Value>(),
        decoded_bytes,
    ]
    .into_iter()
    .try_fold(0usize, checked_accounting_add)
}

impl ControlledFulltextCandidateSet {
    pub(crate) fn into_parts(self) -> (PersistedFulltextCandidateSet, QueryMemoryReservation) {
        (self.candidates, self.memory)
    }
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

fn record_controlled_read(
    midge: &Midge,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    check_controls(controls)?;
    midge.record_query_scan_entry();
    if super::super::query_scan_control::should_cancel_controlled_query_scan() {
        return Err(CassieError::QueryCancelled);
    }
    check_controls(controls)
}

struct ControlledPostingReadContext<'a> {
    midge: &'a Midge,
    tx: &'a cntryl_midge::Transaction,
    relation_id: u64,
    index_id: u64,
    allowed_ids: Option<&'a BTreeSet<String>>,
    manifest: &'a FulltextManifest,
    controls: &'a QueryExecutionControls,
}

struct ControlledCandidateDocumentContext<'a> {
    midge: &'a Midge,
    tx: &'a cntryl_midge::Transaction,
    collection: &'a str,
    row_schema: &'a crate::midge::row_blob::RowSchema,
    relation_id: u64,
    index_id: u64,
    column_store: bool,
    controls: &'a QueryExecutionControls,
}

fn validate_controlled_source_membership(
    context: &ControlledCandidateDocumentContext<'_>,
    source_ids: &BTreeSet<String>,
) -> Result<(), CassieError> {
    for id in source_ids {
        check_controls(context.controls)?;
        let exists = if context.column_store {
            let row_key = Midge::column_store_row_key(context.row_schema.relation_id, id);
            let exists = context
                .tx
                .get(&row_key)
                .map_err(CassieError::from)?
                .is_some();
            record_controlled_read(context.midge, context.controls)?;
            exists
        } else {
            let row_key = Midge::row_key(context.row_schema.relation_id, id);
            if context
                .tx
                .get(&row_key)
                .map_err(CassieError::from)?
                .is_some()
            {
                record_controlled_read(context.midge, context.controls)?;
                true
            } else {
                record_controlled_read(context.midge, context.controls)?;
                let legacy_key = Midge::doc_key(context.collection, id);
                let exists = context
                    .tx
                    .get(&legacy_key)
                    .map_err(CassieError::from)?
                    .is_some();
                record_controlled_read(context.midge, context.controls)?;
                exists
            }
        };
        if !exists {
            return Err(CassieError::Execution(
                "fulltext fallback:missing_candidate_row".to_string(),
            ));
        }
    }
    Ok(())
}

fn load_controlled_document_stats(
    context: &ControlledCandidateDocumentContext<'_>,
    ids: BTreeSet<String>,
    memory: &mut QueryMemoryReservation,
) -> Result<BTreeMap<String, PersistedFulltextDocumentStats>, CassieError> {
    let mut stats = BTreeMap::new();
    for id in ids {
        check_controls(context.controls)?;
        let key = Midge::fulltext_document_stats_key(context.relation_id, context.index_id, &id);
        let raw = context
            .tx
            .get(&key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("fulltext fallback:missing-document-stats".to_string())
            })?;
        record_controlled_read(context.midge, context.controls)?;
        memory.try_grow(key.len().saturating_add(raw.len()))?;
        memory.try_grow(
            raw.len()
                .saturating_mul(2)
                .saturating_add(retained_string_bytes(&id)),
        )?;
        stats.insert(id, codec::decode_document_stats(&raw)?);
    }
    Ok(stats)
}

fn load_controlled_candidate_postings(
    context: &ControlledPostingReadContext<'_>,
    terms: &[String],
    memory: &mut QueryMemoryReservation,
) -> Result<LoadedCandidatePostings, CassieError> {
    let mut loaded = LoadedCandidatePostings {
        ids: BTreeSet::new(),
        source_ids: BTreeSet::new(),
        document_frequency: BTreeMap::new(),
        block_reads: 0,
    };
    for term in terms {
        check_controls(context.controls)?;
        let prefix =
            Midge::fulltext_term_postings_prefix(context.relation_id, context.index_id, term);
        let mut scan = context
            .tx
            .scan(&Query::new().prefix(prefix.into()))
            .map_err(CassieError::from)?;
        let expected = context.manifest.terms.get(term);
        let mut term_documents = BTreeSet::new();
        let mut posting_count = 0usize;
        let mut block_count = 0usize;
        for entry in &mut scan {
            check_controls(context.controls)?;
            let (key, raw) = entry.map_err(CassieError::from)?;
            record_controlled_read(context.midge, context.controls)?;
            memory.try_grow(key.len().saturating_add(raw.len()))?;
            memory.try_grow(
                raw.len()
                    .saturating_mul(2)
                    .saturating_add(BTREE_ENTRY_OVERHEAD),
            )?;
            if key
                != Midge::fulltext_term_postings_block_key(
                    context.relation_id,
                    context.index_id,
                    term,
                    block_count,
                )
            {
                return Err(CassieError::Parse(format!(
                    "non-contiguous fulltext posting blocks for '{term}'"
                )));
            }
            let postings = codec::decode_postings(&raw)?;
            posting_count = posting_count.saturating_add(postings.len());
            for posting in postings {
                let retained = retained_string_bytes(&posting.document_id);
                memory.try_grow(
                    if context
                        .allowed_ids
                        .is_none_or(|allowed| allowed.contains(&posting.document_id))
                    {
                        retained.saturating_mul(2)
                    } else {
                        retained
                    },
                )?;
                if !term_documents.insert(posting.document_id.clone()) {
                    return Err(CassieError::Parse(format!(
                        "duplicate fulltext posting for '{}' in '{term}'",
                        posting.document_id
                    )));
                }
                if context
                    .allowed_ids
                    .is_none_or(|allowed| allowed.contains(&posting.document_id))
                {
                    loaded.ids.insert(posting.document_id);
                }
            }
            block_count = block_count.saturating_add(1);
        }
        if block_count != expected.map_or(0, |integrity| integrity.block_count) {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext posting blocks for '{term}'"
            )));
        }
        if posting_count != expected.map_or(0, |integrity| integrity.posting_count) {
            return Err(CassieError::Parse(format!(
                "incomplete fulltext postings for '{term}'"
            )));
        }
        loaded.block_reads = loaded.block_reads.saturating_add(block_count);
        loaded.source_ids.extend(term_documents.iter().cloned());
        memory
            .try_grow(retained_string_bytes(term).saturating_add(std::mem::size_of::<usize>()))?;
        loaded
            .document_frequency
            .insert(term.clone(), term_documents.len());
    }
    Ok(loaded)
}

impl Midge {
    pub(crate) fn fulltext_candidate_set_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_inner_controlled(collection, index_name, terms, None, controls)
    }

    pub(crate) fn fulltext_candidate_set_for_ids_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: &BTreeSet<String>,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_inner_controlled(
            collection,
            index_name,
            terms,
            Some(allowed_ids),
            controls,
        )
    }

    fn fulltext_candidate_set_inner_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: Option<&BTreeSet<String>>,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        check_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let mut memory = controls.reserve_query_memory(0)?;

        let metadata_key = Self::fulltext_index_key(relation_id, index_id);
        let metadata_raw = tx
            .get(&metadata_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        record_controlled_read(self, controls)?;
        memory.try_grow(metadata_key.len().saturating_add(metadata_raw.len()))?;
        memory.try_grow(metadata_raw.len().saturating_mul(2))?;
        let metadata = codec::decode_metadata(&metadata_raw)?;
        if metadata.version != STATE_VERSION
            || metadata.built_generation != self.collection_generation(&collection)?
        {
            return Err(CassieError::Execution(
                "fulltext fallback:stale-generation".to_string(),
            ));
        }

        let manifest_key =
            Self::fulltext_index_manifest_key(relation_id, index_id, metadata.built_generation);
        let manifest_raw = tx
            .get(&manifest_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Parse("missing fulltext generation manifest".to_string())
            })?;
        record_controlled_read(self, controls)?;
        memory.try_grow(manifest_key.len().saturating_add(manifest_raw.len()))?;
        memory.try_grow(manifest_raw.len().saturating_mul(2))?;
        let manifest = codec::decode_manifest(&manifest_raw)?;
        validate_manifest_header(&metadata, &manifest)?;
        let loaded = load_controlled_candidate_postings(
            &ControlledPostingReadContext {
                midge: self,
                tx: &tx,
                relation_id,
                index_id,
                allowed_ids,
                manifest: &manifest,
                controls,
            },
            terms,
            &mut memory,
        )?;
        let row_schema = self.row_schema(&collection)?;
        let document_context = ControlledCandidateDocumentContext {
            midge: self,
            tx: &tx,
            collection: &collection,
            row_schema: &row_schema,
            relation_id,
            index_id,
            column_store: self.collection_uses_column_store(&collection)?,
            controls,
        };
        validate_controlled_source_membership(&document_context, &loaded.source_ids)?;
        let stats = load_controlled_document_stats(&document_context, loaded.ids, &mut memory)?;
        if metadata.built_generation != self.collection_generation(&collection)? {
            return Err(CassieError::Execution(
                "fulltext fallback:stale-generation".to_string(),
            ));
        }
        check_controls(controls)?;
        Ok(ControlledFulltextCandidateSet {
            candidates: PersistedFulltextCandidateSet {
                total_documents: metadata.total_documents,
                average_document_length: metadata.average_document_length,
                analyzer: metadata.analyzer,
                document_frequency: loaded.document_frequency,
                document_stats: stats,
                posting_block_reads: loaded.block_reads,
            },
            memory,
        })
    }

    pub(crate) fn get_retrieval_document_controlled(
        &self,
        collection: &str,
        id: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<ControlledRetrievalDocument>, CassieError> {
        check_controls(controls)?;
        let collection = self.canonical_collection_name(collection);
        let row_schema = self.row_schema(&collection)?;
        if self.collection_uses_column_store(&collection)? {
            let tx = self.begin_data_readonly_tx_for(&collection)?;
            let row_key = Self::column_store_row_key(row_schema.relation_id, id);
            let exists = tx.get(&row_key).map_err(CassieError::from)?.is_some();
            record_controlled_read(self, controls)?;
            if !exists {
                return Ok(None);
            }

            let initial_bytes = checked_accounting_add(
                std::mem::size_of::<DocumentRef>(),
                checked_accounting_add(id.len(), std::mem::size_of::<serde_json::Value>())?,
            )?;
            let mut memory = controls.reserve_query_memory(initial_bytes)?;
            let mut payload = serde_json::Map::new();
            for field in row_schema.fields.iter().filter(|field| !field.retired) {
                check_controls(controls)?;
                let field_key = super::super::key_encoding::column_store_field_key(
                    row_schema.relation_id,
                    field.field_id,
                    id,
                );
                let raw = tx.get(&field_key).map_err(CassieError::from)?;
                record_controlled_read(self, controls)?;
                let Some(raw) = raw else {
                    continue;
                };
                memory.try_grow(provisional_column_field_bytes(
                    &field.data_type,
                    &field.name,
                    field_key.len(),
                    raw.len(),
                )?)?;
                let value = crate::midge::row_blob::decode_compact_value(&raw)?;
                check_controls(controls)?;
                payload.insert(field.name.clone(), value);
            }
            check_controls(controls)?;
            return Ok(Some(ControlledRetrievalDocument {
                document: DocumentRef {
                    id: id.to_string(),
                    payload: serde_json::Value::Object(payload),
                },
                memory,
            }));
        }

        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let row_key = Self::row_key(row_schema.relation_id, id);
        let mut raw = tx.get(&row_key).map_err(CassieError::from)?;
        record_controlled_read(self, controls)?;
        let raw_key_bytes = if raw.is_some() {
            row_key.len()
        } else {
            let legacy_key = Self::doc_key(&collection, id);
            raw = tx.get(&legacy_key).map_err(CassieError::from)?;
            record_controlled_read(self, controls)?;
            legacy_key.len()
        };
        let Some(raw) = raw else {
            return Ok(None);
        };
        let retained_bytes = raw_key_bytes
            .saturating_add(raw.len().saturating_mul(3))
            .saturating_add(retained_string_bytes(id));
        let memory = controls.reserve_query_memory(retained_bytes)?;
        let payload = super::decode_row(&row_schema, &raw)?;
        check_controls(controls)?;
        Ok(Some(ControlledRetrievalDocument {
            document: DocumentRef {
                id: id.to_string(),
                payload,
            },
            memory,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use serde_json::json;
    use uuid::Uuid;

    use super::{CassieError, Midge, QueryExecutionControls};
    use crate::catalog::{CollectionMeta, CollectionStorageMode};
    use crate::config::CassieRuntimeLimits;
    use crate::types::{DataType, FieldSchema, Schema};

    #[test]
    fn should_reserve_before_decoding_a_column_retrieval_document_given_low_memory() {
        // Arrange
        let path = std::env::temp_dir().join(format!(
            "cassie-controlled-fulltext-column-retrieval-{}",
            Uuid::new_v4()
        ));
        let midge = Midge::new_with_data_dir(&path).expect("create Midge");
        let collection = "controlled_fulltext_column_retrieval";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: false,
            }],
        };
        let metadata = CollectionMeta::new_with_storage_mode(
            collection,
            None,
            CollectionStorageMode::ColumnStore,
        );
        midge
            .create_collection_with_meta(collection, &schema, &metadata)
            .expect("create column collection");
        midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                json!({"body": "alpha".repeat(8_192)}),
            )
            .expect("store column document");
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 1_024,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let before_reads = midge.query_scan_entries_for_diagnostics();

        // Act
        let result = midge.get_retrieval_document_controlled(collection, "doc-1", &controls);
        let Err(error) = result else {
            panic!("column decode must be rejected before retaining the field");
        };
        let reads = midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(reads, 2, "row marker and one raw field read");
        assert_eq!(controls.current_query_memory_bytes(), 0);
        drop(midge);
        let _ = std::fs::remove_dir_all(path);
    }
}
