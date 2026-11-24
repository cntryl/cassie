use std::collections::{BTreeMap, BTreeSet};

use cntryl_midge::Query;

use super::layout::{self, TermBlockValidator};
use super::vocabulary::TermDirectory;
use super::{
    codec, LoadedCandidatePostings, PersistedFulltextCandidateSet, PersistedFulltextDocumentStats,
    STATE_VERSION,
};
use crate::adapter::column_store::validation as column_validation;
use crate::adapter::CassieError;
use crate::adapter::{DocumentRef, Midge, StorageRequestContext};
use cassie_core::request::accounted::json;
use cassie_core::request::RequestMemoryReservation;
use cassie_core::types::DataType;
use cassie_search::query::{CompiledQuery, ResolveError, ResolvedQuery, SearchQueryError};

pub struct ControlledFulltextCandidateSet {
    candidates: PersistedFulltextCandidateSet,
    memory: RequestMemoryReservation,
}

pub struct ControlledRetrievalDocument {
    document: DocumentRef,
    memory: RequestMemoryReservation,
}

impl ControlledRetrievalDocument {
    pub fn into_parts(self) -> (DocumentRef, RequestMemoryReservation) {
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

fn column_decode_expansion_factor(data_type: &DataType) -> usize {
    match data_type {
        DataType::Array(inner) => {
            let inner_factor = column_decode_expansion_factor(inner);
            if inner_factor > 40 {
                inner_factor
            } else {
                40
            }
        }
        DataType::Json | DataType::Jsonb => json::JSON_DECODE_EXPANSION_FACTOR,
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
        json::JSON_MAP_ENTRY_BYTES,
        field_name.len(),
        std::mem::size_of::<serde_json::Value>(),
        decoded_bytes,
    ]
    .into_iter()
    .try_fold(0usize, checked_accounting_add)
}

impl ControlledFulltextCandidateSet {
    pub fn into_parts(self) -> (PersistedFulltextCandidateSet, RequestMemoryReservation) {
        (self.candidates, self.memory)
    }
}

fn grow_memory(
    memory: &mut RequestMemoryReservation,
    additional: usize,
) -> Result<(), CassieError> {
    memory
        .try_grow(additional)
        .map_err(cassie_core::request::accounted::AccountingError::from)?;
    Ok(())
}

fn record_controlled_read(
    midge: &Midge,
    controls: &StorageRequestContext<'_>,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    midge.record_query_scan_entry();
    if super::super::query_scan_control::should_cancel_controlled_query_scan(controls) {
        return Err(CassieError::QueryCanceled);
    }
    controls.check_interruption()
}

struct ControlledPostingReadContext<'a> {
    midge: &'a Midge,
    tx: &'a crate::adapter::AdapterTransaction,
    relation_id: u64,
    index_id: u64,
    allowed_ids: Option<&'a BTreeSet<String>>,
    controls: &'a StorageRequestContext<'a>,
}

struct ControlledCandidateDocumentContext<'a> {
    midge: &'a Midge,
    tx: &'a crate::adapter::AdapterTransaction,
    collection: &'a str,
    row_schema: &'a crate::row_blob::RowSchema,
    relation_id: u64,
    index_id: u64,
    column_store: bool,
    controls: &'a StorageRequestContext<'a>,
}

fn validate_controlled_source_membership(
    context: &ControlledCandidateDocumentContext<'_>,
    source_ids: &BTreeSet<String>,
) -> Result<(), CassieError> {
    for id in source_ids {
        context.controls.check_interruption()?;
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
    memory: &mut RequestMemoryReservation,
) -> Result<BTreeMap<String, PersistedFulltextDocumentStats>, CassieError> {
    let mut stats = BTreeMap::new();
    for id in ids {
        context.controls.check_interruption()?;
        let key = Midge::fulltext_document_stats_key(context.relation_id, context.index_id, &id);
        let raw = context
            .tx
            .get(&key)
            .map_err(CassieError::from)?
            .ok_or_else(|| {
                CassieError::Execution("fulltext fallback:missing-document-stats".to_string())
            })?;
        record_controlled_read(context.midge, context.controls)?;
        grow_memory(memory, key.len().saturating_add(raw.len()))?;
        grow_memory(
            memory,
            raw.len()
                .saturating_mul(2)
                .saturating_add(retained_string_bytes(&id)),
        )?;
        let document = codec::decode_document_stats(&raw)?;
        grow_memory(
            memory,
            document
                .term_positions
                .values()
                .map(|positions| {
                    positions
                        .len()
                        .saturating_mul(std::mem::size_of::<u32>())
                        .saturating_add(std::mem::size_of::<Vec<u32>>())
                })
                .fold(0usize, usize::saturating_add),
        )?;
        stats.insert(id, document);
    }
    Ok(stats)
}

fn load_controlled_candidate_postings(
    context: &ControlledPostingReadContext<'_>,
    terms: &[String],
    memory: &mut RequestMemoryReservation,
) -> Result<LoadedCandidatePostings, CassieError> {
    let mut loaded = LoadedCandidatePostings {
        ids: BTreeSet::new(),
        source_ids: BTreeSet::new(),
        document_frequency: BTreeMap::new(),
        block_reads: 0,
    };
    for term in terms {
        context.controls.check_interruption()?;
        let prefix =
            Midge::fulltext_term_postings_prefix(context.relation_id, context.index_id, term);
        let mut scan = context
            .tx
            .scan(&Query::new().prefix(prefix.into()))
            .map_err(CassieError::from)?;
        let record_key =
            Midge::fulltext_term_record_key(context.relation_id, context.index_id, term);
        let record_raw = context.tx.get(&record_key).map_err(CassieError::from)?;
        record_controlled_read(context.midge, context.controls)?;
        let record = record_raw
            .map(|raw| {
                grow_memory(memory, record_key.len().saturating_add(raw.len()))?;
                codec::decode_term_record(&raw)
            })
            .transpose()?;
        let mut validator = TermBlockValidator::new(term, context.relation_id, context.index_id);
        let mut term_documents = BTreeSet::new();
        for entry in &mut scan {
            context.controls.check_interruption()?;
            let (key, raw) = entry.map_err(CassieError::from)?;
            record_controlled_read(context.midge, context.controls)?;
            grow_memory(memory, key.len().saturating_add(raw.len()))?;
            grow_memory(
                memory,
                raw.len()
                    .saturating_mul(2)
                    .saturating_add(BTREE_ENTRY_OVERHEAD),
            )?;
            let postings = codec::decode_postings(&raw)?;
            validator.accept(&key, &postings)?;
            for posting in postings {
                let retained = retained_string_bytes(&posting.document_id);
                grow_memory(
                    memory,
                    if context
                        .allowed_ids
                        .is_none_or(|allowed| allowed.contains(&posting.document_id))
                    {
                        retained.saturating_mul(2)
                    } else {
                        retained
                    },
                )?;
                term_documents.insert(posting.document_id.clone());
                if context
                    .allowed_ids
                    .is_none_or(|allowed| allowed.contains(&posting.document_id))
                {
                    loaded.ids.insert(posting.document_id);
                }
            }
        }
        let block_count = validator.block_count();
        validator.finish(record.as_ref())?;
        loaded.block_reads = loaded.block_reads.saturating_add(block_count);
        loaded.source_ids.extend(term_documents.iter().cloned());
        grow_memory(
            memory,
            retained_string_bytes(term).saturating_add(std::mem::size_of::<usize>()),
        )?;
        loaded
            .document_frequency
            .insert(term.clone(), term_documents.len());
    }
    Ok(loaded)
}

impl Midge {
    /// # Errors
    ///
    /// Returns an interruption, resource-limit, or storage error when the requested read fails.
    pub fn fulltext_candidate_set_with_request(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        controls: &StorageRequestContext<'_>,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_inner_controlled(collection, index_name, terms, None, controls)
    }

    /// # Errors
    ///
    /// Returns an interruption, resource-limit, or storage error when the requested read fails.
    pub fn fulltext_candidate_set_for_ids_with_request(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: &BTreeSet<String>,
        controls: &StorageRequestContext<'_>,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_inner_controlled(
            collection,
            index_name,
            terms,
            Some(allowed_ids),
            controls,
        )
    }

    /// Reads the candidates of a `'query'`-syntax search: expands its prefix
    /// and typo items against the index vocabulary inside the same read
    /// transaction as the posting and statistics reads, then loads the postings
    /// of the positive terms.
    ///
    /// Returns the candidates, the resolved query and the reservation that
    /// accounts the resolved query.
    /// # Errors
    ///
    /// Returns an interruption, resource-limit, or storage error when the requested read fails.
    pub fn fulltext_candidate_set_for_query_with_request(
        &self,
        collection: &str,
        index_name: &str,
        query: &CompiledQuery,
        controls: &StorageRequestContext<'_>,
    ) -> Result<
        (
            ControlledFulltextCandidateSet,
            ResolvedQuery,
            RequestMemoryReservation,
        ),
        CassieError,
    > {
        let mut resolved = None;
        let mut resolved_memory = None;
        let candidates = self.fulltext_candidate_set_resolving_controlled(
            collection,
            index_name,
            None,
            controls,
            &mut |directory| {
                let query = query.resolve(directory).map_err(|error| match error {
                    ResolveError::Vocabulary(error) => error,
                    ResolveError::Query(SearchQueryError::InvalidParameter(message)) => {
                        CassieError::InvalidParameterValue(message)
                    }
                    ResolveError::Query(SearchQueryError::ResourceLimit(message)) => {
                        CassieError::ResourceLimit(message)
                    }
                })?;
                resolved_memory = Some(controls.reserve(query.estimated_bytes())?);
                let terms = query.positive_terms().to_vec();
                resolved = Some(query);
                Ok(terms)
            },
        )?;
        match (resolved, resolved_memory) {
            (Some(resolved), Some(memory)) => Ok((candidates, resolved, memory)),
            _ => Err(CassieError::Execution(
                "fulltext fallback:unresolved-query".to_string(),
            )),
        }
    }

    fn fulltext_candidate_set_inner_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: Option<&BTreeSet<String>>,
        controls: &StorageRequestContext<'_>,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        self.fulltext_candidate_set_resolving_controlled(
            collection,
            index_name,
            allowed_ids,
            controls,
            &mut |_| Ok(terms.to_vec()),
        )
    }

    fn fulltext_candidate_set_resolving_controlled(
        &self,
        collection: &str,
        index_name: &str,
        allowed_ids: Option<&BTreeSet<String>>,
        controls: &StorageRequestContext<'_>,
        resolve_terms: &mut dyn FnMut(&TermDirectory<'_>) -> Result<Vec<String>, CassieError>,
    ) -> Result<ControlledFulltextCandidateSet, CassieError> {
        controls.check_interruption()?;
        let collection = self.canonical_collection_name(collection);
        let index = self
            .get_index(&collection, index_name)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        let (relation_id, index_id) = Self::fulltext_storage_ids(&index)?;
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let mut memory = controls.reserve(0)?;

        let metadata_key = Self::fulltext_index_key(relation_id, index_id);
        let metadata_raw = tx
            .get(&metadata_key)
            .map_err(CassieError::from)?
            .ok_or_else(|| CassieError::Execution("fulltext fallback:missing-index".to_string()))?;
        record_controlled_read(self, controls)?;
        grow_memory(
            &mut memory,
            metadata_key.len().saturating_add(metadata_raw.len()),
        )?;
        grow_memory(&mut memory, metadata_raw.len().saturating_mul(2))?;
        let metadata = codec::decode_metadata(&metadata_raw)?;
        if metadata.version != STATE_VERSION
            || metadata.built_generation != self.collection_generation(&collection)?
        {
            return Err(CassieError::Execution(
                "fulltext fallback:stale-generation".to_string(),
            ));
        }

        let terms = resolve_terms(&TermDirectory {
            tx: &tx,
            relation_id,
            index_id,
            controls,
        })?;
        let loaded = load_controlled_candidate_postings(
            &ControlledPostingReadContext {
                midge: self,
                tx: &tx,
                relation_id,
                index_id,
                allowed_ids,
                controls,
            },
            &terms,
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
        controls.check_interruption()?;
        Ok(ControlledFulltextCandidateSet {
            candidates: PersistedFulltextCandidateSet {
                total_documents: metadata.total_documents,
                average_document_length: layout::average_document_length(
                    metadata.total_document_length,
                    metadata.documents_with_text,
                ),
                analyzer: metadata.analyzer,
                document_frequency: loaded.document_frequency,
                document_stats: stats,
                posting_block_reads: loaded.block_reads,
            },
            memory,
        })
    }

    /// # Errors
    ///
    /// Returns an interruption, resource-limit, or storage error when the requested read fails.
    pub fn get_retrieval_document_with_request(
        &self,
        collection: &str,
        id: &str,
        controls: &StorageRequestContext<'_>,
    ) -> Result<Option<ControlledRetrievalDocument>, CassieError> {
        controls.check_interruption()?;
        let collection = self.canonical_collection_name(collection);
        let row_schema = self.row_schema(&collection)?;
        if self.collection_uses_column_store(&collection)? {
            let tx = self.begin_data_readonly_tx_for(&collection)?;
            let row_key = Self::column_store_row_key(row_schema.relation_id, id);
            let marker = tx.get(&row_key).map_err(CassieError::from)?;
            record_controlled_read(self, controls)?;
            let Some(marker) = marker else {
                return Ok(None);
            };
            let marker_version = column_validation::decode_marker(&collection, id, &marker)?;

            let initial_bytes = checked_accounting_add(
                std::mem::size_of::<DocumentRef>(),
                checked_accounting_add(id.len(), std::mem::size_of::<serde_json::Value>())?,
            )?;
            let mut memory = controls.reserve(initial_bytes)?;
            let mut payload = serde_json::Map::new();
            for field in row_schema.fields.iter().filter(|field| !field.retired) {
                controls.check_interruption()?;
                let field_key = super::super::key_encoding::column_store_field_key(
                    row_schema.relation_id,
                    field.field_id,
                    id,
                );
                let raw = tx.get(&field_key).map_err(CassieError::from)?;
                record_controlled_read(self, controls)?;
                let Some(raw) = raw else {
                    if column_validation::field_is_required(field, marker_version) {
                        return Err(column_validation::missing_field(&collection, id, field));
                    }
                    continue;
                };
                grow_memory(
                    &mut memory,
                    provisional_column_field_bytes(
                        &field.data_type,
                        &field.name,
                        field_key.len(),
                        super::super::column_store::column_payload_logical_len(&raw),
                    )?,
                )?;
                let Some(value) = column_validation::decode_field(
                    &collection,
                    id,
                    field,
                    &raw,
                    tx.value_hooks(),
                )?
                else {
                    continue;
                };
                controls.check_interruption()?;
                payload.insert(field.name.clone(), value);
            }
            controls.check_interruption()?;
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
        let retained_bytes = super::super::streaming_scans::provisional_controlled_document_bytes(
            &row_schema,
            None,
            false,
            raw_key_bytes,
            raw.len(),
        )?;
        let memory = controls.reserve(retained_bytes)?;
        let payload = super::decode_row_with_hooks(&row_schema, &raw, tx.value_hooks())?;
        controls.check_interruption()?;
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
    use serde_json::json;

    use super::{CassieError, Midge, StorageRequestContext};
    use cassie_core::catalog::declarations::CollectionStorageMode;
    use cassie_core::catalog::storage_metadata::CollectionMeta;
    use cassie_core::request::{Interruption, RequestCancellation, RequestMemory};
    use cassie_core::types::{DataType, FieldSchema, Schema};

    #[test]
    fn should_reserve_before_decoding_a_column_retrieval_document_given_low_memory() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
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
        let memory = RequestMemory::new(1_024);
        let cancellation = RequestCancellation::new();
        let controls =
            StorageRequestContext::new(None, Interruption::new(None, &cancellation), &memory);
        let before_reads = midge.query_scan_entries_for_diagnostics();

        // Act
        let result = midge.get_retrieval_document_with_request(collection, "doc-1", &controls);
        let Err(error) = result else {
            panic!("column decode must be rejected before retaining the field");
        };
        let reads = midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(reads, 2, "row marker and one raw field read");
        assert_eq!(memory.current_bytes(), 0);
    }
}
