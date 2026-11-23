use super::{
    check_document_write_failure_point, decode_row, encode_row, key_encoding, CassieError,
    DataType, DocumentRef, DocumentWriteFailurePoint, FieldConstraint, IndexKind, IndexMeta, Midge,
    RowSchema, Schema, Uuid,
};
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "documents/commit.rs"]
mod commit;
#[path = "documents/options.rs"]
mod options;
#[path = "documents/ordered_scan.rs"]
mod ordered_scan;
#[path = "documents/read.rs"]
mod read;
#[path = "documents/replay.rs"]
mod replay;
#[path = "documents/report.rs"]
mod report;
#[path = "documents/unique_reservations.rs"]
mod unique_reservations;
pub(crate) use super::projections::ReplayBatchCommit;
pub(super) use commit::document_write_retry_delay;
pub(crate) use options::DocumentWriteBatchOptions;
pub(crate) use ordered_scan::OrderedRowScanRequest;
pub(crate) use report::{DocumentWriteBatchReport, RollupSourceChangeReport};

#[derive(Debug, Clone)]
pub(crate) enum DocumentWriteOp {
    Put {
        id: String,
        payload: serde_json::Value,
    },
    Delete {
        id: String,
    },
}

#[derive(Debug)]
struct PreparedWrite {
    id: String,
    row_blob: Option<Vec<u8>>,
    payload: Option<serde_json::Value>,
    normalized_records: Vec<crate::embeddings::NormalizedVectorRecord>,
}

use super::vector_indexes::{VectorBatchWork, VectorFieldWrites};

struct DocumentWriteBatchContext {
    schema: Schema,
    row_schema: RowSchema,
    target_generation: u64,
    uses_column_store: bool,
    vector_indexes: Vec<crate::embeddings::VectorIndexRecord>,
    vector_fields: Vec<String>,
    unique_constraints: Vec<FieldConstraint>,
    unique_scalar_indexes: Vec<IndexMeta>,
    scalar_indexes: Vec<super::IndexMeta>,
    time_series_indexes: Vec<super::IndexMeta>,
    fulltext_indexes: Vec<super::IndexMeta>,
    needs_existing_payload: bool,
}

#[derive(Default)]
struct PreparedDocumentWriteBatches {
    reports: BTreeMap<String, DocumentWriteBatchReport>,
    changed_collections: Vec<String>,
    vector: VectorBatchWork,
    fulltext_indexes_by_collection: BTreeMap<String, Vec<IndexMeta>>,
}

struct ExistingDocumentState {
    payload: Option<serde_json::Value>,
    row_exists: bool,
    legacy_exists: bool,
}

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_document(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<String, CassieError> {
        let storage_collection = self.canonical_collection_name(collection);
        let doc_id = id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let mut options = DocumentWriteBatchOptions::sync(self.write_options_sync());
        options.normalized_vector_collection = Some(collection.to_string());
        self.apply_document_write_batch_with_options(
            &storage_collection,
            vec![DocumentWriteOp::Put {
                id: doc_id.clone(),
                payload,
            }],
            &options,
        )?;
        Ok(doc_id)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_documents(
        &self,
        collection: &str,
        documents: Vec<(Option<String>, serde_json::Value)>,
    ) -> Result<Vec<String>, CassieError> {
        let storage_collection = self.canonical_collection_name(collection);
        let ops = documents
            .into_iter()
            .map(|(id, payload)| DocumentWriteOp::Put {
                id: id.unwrap_or_else(|| Uuid::now_v7().to_string()),
                payload,
            })
            .collect::<Vec<_>>();
        let mut options = DocumentWriteBatchOptions::sync(self.write_options_sync());
        options.normalized_vector_collection = Some(collection.to_string());
        let report =
            self.apply_document_write_batch_with_options(&storage_collection, ops, &options)?;
        Ok(report.ids)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_document(
        &self,
        collection: &str,
        id: &str,
    ) -> Result<Option<DocumentRef>, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let row_schema = self.row_schema(&collection)?;
        if self.collection_uses_column_store(&collection)? {
            let tx = self.begin_data_readonly_tx_for(&collection)?;
            let Some(payload) =
                Self::load_column_store_document_from_tx(&tx, &collection, id, &row_schema)?
            else {
                Self::reject_orphan_column_store_fields(&tx, &collection, id, &row_schema)?;
                return Ok(None);
            };
            return Ok(Some(DocumentRef {
                id: id.to_string(),
                payload,
            }));
        }

        let tx = self.begin_data_readonly_tx_for(&collection)?;
        let payload = match tx
            .get(&Self::row_key(row_schema.relation_id, id))
            .map_err(CassieError::from)?
        {
            Some(payload) => Some(payload),
            None => tx
                .get(&Self::doc_key(&collection, id))
                .map_err(CassieError::from)?,
        };

        let Some(payload) = payload else {
            return Ok(None);
        };
        let payload = decode_row(&row_schema, &payload)?;

        Ok(Some(DocumentRef {
            id: id.to_string(),
            payload,
        }))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_document(&self, collection: &str, id: &str) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let _row_schema = self.row_schema(&collection)?;
        let report = self.apply_document_write_batch(
            &collection,
            vec![DocumentWriteOp::Delete { id: id.to_string() }],
        )?;
        Ok(report.row_delta < 0)
    }

    pub(crate) fn put_document_with_stats_and_options(
        &self,
        collection: &str,
        id: Option<String>,
        payload: serde_json::Value,
        options: &DocumentWriteBatchOptions,
    ) -> Result<(String, crate::runtime::ProjectionWriteStats, i64), CassieError> {
        let storage_collection = self.canonical_collection_name(collection);
        let doc_id = id.unwrap_or_else(|| Uuid::now_v7().to_string());
        let mut options = options.clone();
        options.normalized_vector_collection = Some(collection.to_string());
        let report = self.apply_document_write_batch_with_options(
            &storage_collection,
            vec![DocumentWriteOp::Put {
                id: doc_id.clone(),
                payload,
            }],
            &options,
        )?;
        Ok((doc_id, report.stats, report.row_delta))
    }

    pub(crate) fn delete_document_with_stats_and_options(
        &self,
        collection: &str,
        id: &str,
        options: &DocumentWriteBatchOptions,
    ) -> Result<(bool, crate::runtime::ProjectionWriteStats, i64), CassieError> {
        let collection = self.canonical_collection_name(collection);
        let report = self.apply_document_write_batch_with_options(
            &collection,
            vec![DocumentWriteOp::Delete { id: id.to_string() }],
            options,
        )?;
        Ok((report.row_delta < 0, report.stats, report.row_delta))
    }

    pub(crate) fn apply_document_write_batch(
        &self,
        collection: &str,
        operations: Vec<DocumentWriteOp>,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        self.apply_document_write_batch_with_options(
            collection,
            operations,
            &DocumentWriteBatchOptions::sync(self.write_options_sync()),
        )
    }

    pub(crate) fn apply_document_write_batch_with_options(
        &self,
        collection: &str,
        operations: Vec<DocumentWriteOp>,
        options: &DocumentWriteBatchOptions,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        if operations.is_empty() && !options.has_side_writes() {
            return Ok(DocumentWriteBatchReport::default());
        }
        let collection = self.canonical_collection_name(collection);
        let mut writes = BTreeMap::new();
        writes.insert(collection.clone(), operations);
        let mut reports = self.apply_document_write_batches_with_options(&writes, options)?;
        Ok(reports.remove(&collection).unwrap_or_default())
    }

    pub(crate) fn apply_document_write_batches_with_options(
        &self,
        writes: &BTreeMap<String, Vec<DocumentWriteOp>>,
        options: &DocumentWriteBatchOptions,
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        self.apply_document_write_batches_with_storage_overlay(writes, options, None, None)
    }

    pub(crate) fn apply_document_write_batches_with_transactional_ddl(
        &self,
        writes: &BTreeMap<String, Vec<DocumentWriteOp>>,
        options: &DocumentWriteBatchOptions,
        overlay: &super::StorageOverlay,
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        self.apply_document_write_batches_with_storage_overlay(writes, options, Some(overlay), None)
    }

    pub(crate) fn apply_document_write_batches_with_transactional_ddl_and_schema(
        &self,
        writes: &BTreeMap<String, Vec<DocumentWriteOp>>,
        options: &DocumentWriteBatchOptions,
        overlay: &super::StorageOverlay,
        schema: &Schema,
        row_schema: &RowSchema,
        constraints: &[FieldConstraint],
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        self.apply_document_write_batches_with_storage_overlay(
            writes,
            options,
            Some(overlay),
            Some((schema, row_schema, constraints)),
        )
    }

    fn apply_document_write_batches_with_storage_overlay(
        &self,
        writes: &BTreeMap<String, Vec<DocumentWriteOp>>,
        options: &DocumentWriteBatchOptions,
        overlay: Option<&super::StorageOverlay>,
        schema_override: Option<(&Schema, &RowSchema, &[FieldConstraint])>,
    ) -> Result<BTreeMap<String, DocumentWriteBatchReport>, CassieError> {
        if writes.is_empty() && options.additional_data_writes.is_empty() {
            if let Some(overlay) = overlay {
                let database = self.default_database.clone();
                self.commit_storage_overlay(&database, overlay)?;
            }
            return Ok(BTreeMap::new());
        }
        let mut canonical_writes = BTreeMap::<String, Vec<DocumentWriteOp>>::new();
        for (collection, operations) in writes {
            canonical_writes
                .entry(self.canonical_collection_name(collection))
                .or_default()
                .extend(operations.iter().cloned());
        }

        let mut attempts = 0u8;
        loop {
            attempts = attempts.saturating_add(1);

            let mut prepared_writes = Vec::new();
            let mut collections = canonical_writes
                .iter()
                .filter(|(_, operations)| !operations.is_empty() || options.replay.is_some())
                .map(|(collection, operations)| (collection.clone(), operations.clone()))
                .collect::<Vec<_>>();
            collections.sort_by(|(left, _), (right, _)| {
                left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase())
            });

            let write_gates = collections
                .iter()
                .map(|(collection, _)| self.collection_write_gate(collection))
                .collect::<Vec<_>>();
            let mut write_guards = Vec::with_capacity(write_gates.len());
            for write_gate in &write_gates {
                write_guards.push(write_gate.lock());
            }

            let collection_names = || {
                collections
                    .iter()
                    .map(|(collection, _)| collection.as_str())
            };
            let database = options.database_for(collection_names().next(), &self.default_database);
            options::require_single_database(collection_names(), &database)?;
            let _overlay_scope = if let Some(overlay) = overlay {
                let overlay = Arc::new(parking_lot::Mutex::new(overlay.clone()));
                Some(self.enter_transactional_ddl_scope(&database, overlay, false)?)
            } else {
                None
            };
            for (collection, operations) in &collections {
                let context = self.document_write_batch_context_with_schema_override(
                    collection,
                    options,
                    schema_override,
                )?;
                let prepared =
                    Self::prepare_document_writes(collection, operations.clone(), &context)?;
                prepared_writes.push((collection.clone(), context, prepared));
            }

            let mut tx = self
                .begin_database_adapter_tx(&database, cntryl_midge::TransactionMode::ReadWrite)?;
            tx.set_conflict_policy(cntryl_midge::ConflictPolicy::AbortOnWriteConflict);
            if let Some(overlay) = overlay {
                Self::apply_storage_overlay_to_transaction(&mut tx, overlay)?;
            }
            let PreparedDocumentWriteBatches {
                reports,
                changed_collections,
                vector,
                fulltext_indexes_by_collection,
            } = self.apply_prepared_document_write_batches(&mut tx, prepared_writes, options)?;

            if let Some(replay) = &options.replay {
                self.stage_projection_replay_in_tx(&mut tx, replay)?;
            }

            if changed_collections.is_empty() && !options.has_side_writes() {
                tx.rollback().map_err(CassieError::from)?;
                if let Some(overlay) = overlay {
                    self.commit_storage_overlay(&database, overlay)?;
                }
                return Ok(reports);
            }

            match self.finish_document_write_batches(
                options,
                tx,
                reports,
                changed_collections,
                &vector,
                &fulltext_indexes_by_collection,
            ) {
                Ok(reports) => return Ok(reports),
                Err(error) if options.replay.is_some() => return Err(error),
                Err(error) => match commit::document_write_retry_delay(&error, attempts) {
                    Some(delay) => {
                        // Releases only this attempt's own guards. The gates are
                        // reentrant, so any gates an outer caller already holds
                        // stay held during the backoff.
                        drop(write_guards);
                        std::thread::sleep(delay);
                    }
                    None => return Err(error),
                },
            }
        }
    }

    fn apply_prepared_document_write_batches(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        prepared_writes: Vec<(String, DocumentWriteBatchContext, Vec<PreparedWrite>)>,
        options: &DocumentWriteBatchOptions,
    ) -> Result<PreparedDocumentWriteBatches, CassieError> {
        let mut applied = PreparedDocumentWriteBatches::default();
        for (collection, context, prepared_collection) in prepared_writes {
            let (report, vector_writes) = self.apply_prepared_collection_writes(
                tx,
                &collection,
                &context,
                prepared_collection,
                options,
            )?;
            if report.has_changes() {
                self.refresh_vector_index_states_in_tx(
                    tx,
                    &context.row_schema,
                    &context.vector_indexes,
                    &vector_writes,
                    (context.target_generation, &mut applied.vector.maintenance),
                )?;
                applied
                    .vector
                    .writes_by_collection
                    .insert(collection.clone(), vector_writes);
                applied
                    .fulltext_indexes_by_collection
                    .insert(collection.clone(), context.fulltext_indexes.clone());
                applied.changed_collections.push(collection.clone());
            }
            applied.reports.insert(collection, report);
        }
        Ok(applied)
    }

    fn apply_storage_overlay_to_transaction(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        overlay: &super::StorageOverlay,
    ) -> Result<(), CassieError> {
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
        Ok(())
    }

    fn apply_prepared_collection_writes(
        &self,
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared_collection: Vec<PreparedWrite>,
        options: &DocumentWriteBatchOptions,
    ) -> Result<(DocumentWriteBatchReport, Vec<VectorFieldWrites>), CassieError> {
        let mut report = DocumentWriteBatchReport {
            relation_id: context.row_schema.relation_id,
            ..DocumentWriteBatchReport::default()
        };
        let normalized_vector_collection = options
            .normalized_vector_collection
            .as_deref()
            .and_then(|candidate| {
                (self.canonical_collection_name(candidate) == collection).then_some(candidate)
            });
        let vector_writes = Self::vector_field_writes(
            &context.vector_indexes,
            &prepared_collection
                .iter()
                .map(|write| (write.id.clone(), write.normalized_records.clone()))
                .collect::<Vec<_>>(),
            normalized_vector_collection,
        );
        Self::release_unique_reservations_for_batch(tx, collection, context, &prepared_collection)?;
        for prepared in prepared_collection {
            Self::apply_prepared_document_write(
                tx,
                collection,
                context,
                prepared,
                normalized_vector_collection,
                &options.rollup_change_fields,
                &mut report,
            )?;
        }
        Ok((report, vector_writes))
    }

    fn document_write_batch_context_with_schema_override(
        &self,
        collection: &str,
        options: &DocumentWriteBatchOptions,
        schema_override: Option<(&Schema, &RowSchema, &[FieldConstraint])>,
    ) -> Result<DocumentWriteBatchContext, CassieError> {
        let mut context = self.document_write_batch_context(collection, options)?;
        if let Some((schema, row_schema, constraints)) = schema_override {
            context.schema = schema.clone();
            context.row_schema = row_schema.clone();
            context.unique_constraints = constraints
                .iter()
                .filter(|constraint| {
                    crate::catalog::enforces_single_column_uniqueness(constraint, constraints)
                })
                .cloned()
                .collect();
        }
        Ok(context)
    }

    fn document_write_batch_context(
        &self,
        collection: &str,
        options: &DocumentWriteBatchOptions,
    ) -> Result<DocumentWriteBatchContext, CassieError> {
        let capture_rollup_changes = !options.rollup_change_fields.is_empty();
        let schema = self
            .collection_schema(collection)
            .ok_or_else(|| CassieError::CollectionNotFound(collection.to_string()))?;
        let row_schema = self.row_schema(collection)?;
        let target_generation = self.collection_generation(collection)?.saturating_add(1);
        let uses_column_store = self.collection_uses_column_store(collection)?;
        let vector_indexes = self
            .list_vector_index_definitions_canonical()?
            .into_iter()
            .filter(|index| index.collection == collection)
            .collect::<Vec<_>>();
        let vector_fields = vector_indexes
            .iter()
            .map(|index| index.field.clone())
            .collect::<Vec<_>>();
        let indexes = self.list_indexes()?;
        let scalar_indexes = indexes
            .iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::Scalar)
            .cloned()
            .collect::<Vec<_>>();
        let unique_scalar_indexes = indexes
            .iter()
            .filter(|index| {
                index.collection == collection && index.kind == IndexKind::Scalar && index.unique
            })
            .cloned()
            .collect::<Vec<_>>();
        let time_series_indexes = indexes
            .iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::TimeSeries)
            .cloned()
            .collect::<Vec<_>>();
        let fulltext_indexes = indexes
            .iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::FullText)
            .cloned()
            .collect::<Vec<_>>();
        let loaded = self.load_constraints(collection)?;
        let constraints = loaded
            .iter()
            .filter(|constraint| {
                crate::catalog::enforces_single_column_uniqueness(constraint, &loaded)
            })
            .cloned()
            .collect::<Vec<_>>();
        let needs_existing_payload = !constraints.is_empty()
            || !unique_scalar_indexes.is_empty()
            || !scalar_indexes.is_empty()
            || !time_series_indexes.is_empty()
            || capture_rollup_changes;
        Ok(DocumentWriteBatchContext {
            schema,
            row_schema,
            target_generation,
            uses_column_store,
            vector_indexes,
            vector_fields,
            unique_constraints: constraints,
            unique_scalar_indexes,
            scalar_indexes,
            time_series_indexes,
            fulltext_indexes,
            needs_existing_payload,
        })
    }

    fn prepare_document_writes(
        collection: &str,
        operations: Vec<DocumentWriteOp>,
        context: &DocumentWriteBatchContext,
    ) -> Result<Vec<PreparedWrite>, CassieError> {
        let mut prepared = Vec::with_capacity(operations.len());
        for operation in operations {
            match operation {
                DocumentWriteOp::Put { id, payload } => {
                    super::row_id::reject_nul_row_id(&id)?;
                    Self::validate_document(&context.schema, &payload)?;
                    let row_blob = encode_row(&context.row_schema, &payload)?;
                    let normalized_records = context
                        .vector_indexes
                        .iter()
                        .map(|index| {
                            Self::normalized_vector_record_from_value(
                                collection,
                                &index.field,
                                &id,
                                index.metadata.dimensions,
                                index.metadata.metric,
                                payload.get(&index.field),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>();

                    prepared.push(PreparedWrite {
                        id,
                        row_blob: Some(row_blob),
                        payload: Some(payload),
                        normalized_records,
                    });
                }
                DocumentWriteOp::Delete { id } => {
                    prepared.push(PreparedWrite {
                        id,
                        row_blob: None,
                        payload: None,
                        normalized_records: Vec::new(),
                    });
                }
            }
        }
        Ok(prepared)
    }

    fn apply_prepared_document_write(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: PreparedWrite,
        normalized_vector_collection: Option<&str>,
        rollup_change_fields: &[String],
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        let existing = Self::existing_document_state_for_prepared_write(
            tx,
            collection,
            context,
            &prepared.id,
        )?;
        if !rollup_change_fields.is_empty() {
            report.rollup_changes.record(
                existing.payload.as_ref(),
                prepared.payload.as_ref(),
                rollup_change_fields,
            );
        }
        if prepared.row_blob.is_some() {
            return Self::apply_prepared_put(
                tx,
                collection,
                context,
                prepared,
                normalized_vector_collection,
                &existing,
                report,
            );
        }
        Self::apply_prepared_delete(tx, collection, context, &prepared, &existing, report)
    }

    fn existing_document_state_for_prepared_write(
        tx: &crate::midge::adapter::AdapterTransaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        id: &str,
    ) -> Result<ExistingDocumentState, CassieError> {
        let legacy_key = Self::doc_key(collection, id);
        if context.uses_column_store {
            let payload = if context.needs_existing_payload {
                Self::load_column_store_document_from_tx(tx, collection, id, &context.row_schema)?
            } else {
                None
            };
            let row_exists = if context.needs_existing_payload {
                payload.is_some()
            } else {
                tx.get(&Self::column_store_row_key(
                    context.row_schema.relation_id,
                    id,
                ))
                .map_err(CassieError::from)?
                .is_some()
            };
            let legacy_exists = tx.get(&legacy_key).map_err(CassieError::from)?.is_some();
            return Ok(ExistingDocumentState {
                payload,
                row_exists,
                legacy_exists,
            });
        }

        let row_raw = tx
            .get(&Self::row_key(context.row_schema.relation_id, id))
            .map_err(CassieError::from)?;
        let legacy_raw = tx.get(&legacy_key).map_err(CassieError::from)?;
        let payload = if context.needs_existing_payload {
            match (row_raw.as_ref(), legacy_raw.as_ref()) {
                (Some(raw), _) | (None, Some(raw)) => Some(decode_row(&context.row_schema, raw)?),
                (None, None) => None,
            }
        } else {
            None
        };
        Ok(ExistingDocumentState {
            payload,
            row_exists: row_raw.is_some(),
            legacy_exists: legacy_raw.is_some(),
        })
    }

    fn apply_prepared_put(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        mut prepared: PreparedWrite,
        normalized_vector_collection: Option<&str>,
        existing: &ExistingDocumentState,
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        let row_blob = prepared
            .row_blob
            .expect("prepared put operation must include row blob");
        let payload = prepared
            .payload
            .expect("prepared put operation must include payload");
        // Existing payloads are decoded from row blobs and are already canonical.
        let index_payload = if context.scalar_indexes.is_empty()
            && context.unique_constraints.is_empty()
            && context.time_series_indexes.is_empty()
        {
            std::borrow::Cow::Borrowed(&payload)
        } else {
            super::scalar_indexes::scalar_index_canonical_payload(&context.row_schema, &payload)
        };
        Self::sync_unique_reservations_for_document(
            tx,
            collection,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            Some(index_payload.as_ref()),
        )?;
        let row_key = Self::row_key(context.row_schema.relation_id, &prepared.id);
        let legacy_key = Self::doc_key(collection, &prepared.id);
        let replacing = existing.row_exists || existing.legacy_exists;
        let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
            tx,
            &context.row_schema,
            &prepared.id,
            &context.vector_fields,
        )?;
        report.stats.index_deletes = report
            .stats
            .index_deletes
            .saturating_add(u64::try_from(normalized_deleted).unwrap_or(0));
        if context.uses_column_store {
            Self::write_column_store_document_to_tx(
                tx,
                collection,
                &prepared.id,
                &payload,
                &context.row_schema,
            )?;
        } else {
            tx.put(row_key, row_blob, None).map_err(CassieError::from)?;
        }
        check_document_write_failure_point(DocumentWriteFailurePoint::Row)?;
        Self::write_document_hash_to_tx(
            tx,
            collection,
            &prepared.id,
            &context.row_schema,
            &payload,
        )?;
        if existing.legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        if let Some(normalized_vector_collection) = normalized_vector_collection {
            for record in &mut prepared.normalized_records {
                record.collection = normalized_vector_collection.to_string();
            }
        }
        Self::write_normalized_vector_records(
            tx,
            &context.row_schema,
            &prepared.normalized_records,
        )?;
        check_document_write_failure_point(DocumentWriteFailurePoint::NormalizedVector)?;
        let index_changes = Self::sync_secondary_indexes_for_write(
            tx,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            Some(index_payload.as_ref()),
        )?;
        report.changed_ids.push(prepared.id.clone());
        report.ids.push(prepared.id);
        report.stats.row_puts = report.stats.row_puts.saturating_add(1);
        report.stats.index_puts = report.stats.index_puts.saturating_add(
            u64::try_from(
                prepared
                    .normalized_records
                    .len()
                    .saturating_add(index_changes.1),
            )
            .unwrap_or(0),
        );
        report.stats.index_deletes = report
            .stats
            .index_deletes
            .saturating_add(u64::try_from(index_changes.0).unwrap_or(0));
        report.stats.metadata_puts = report.stats.metadata_puts.saturating_add(1);
        if !replacing {
            report.row_delta = report.row_delta.saturating_add(1);
        }
        Ok(())
    }

    fn apply_prepared_delete(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        collection: &str,
        context: &DocumentWriteBatchContext,
        prepared: &PreparedWrite,
        existing: &ExistingDocumentState,
        report: &mut DocumentWriteBatchReport,
    ) -> Result<(), CassieError> {
        Self::sync_unique_reservations_for_document(
            tx,
            collection,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            None,
        )?;
        let row_key = Self::row_key(context.row_schema.relation_id, &prepared.id);
        let legacy_key = Self::doc_key(collection, &prepared.id);
        if existing.row_exists && context.uses_column_store {
            Self::delete_column_store_document_to_tx(
                tx,
                collection,
                &prepared.id,
                &context.row_schema,
            )?;
        } else if existing.row_exists {
            tx.delete(row_key).map_err(CassieError::from)?;
        }
        if existing.legacy_exists {
            tx.delete(legacy_key).map_err(CassieError::from)?;
        }
        check_document_write_failure_point(DocumentWriteFailurePoint::Row)?;
        if existing.row_exists || existing.legacy_exists {
            Self::delete_document_hash_to_tx(tx, collection, &prepared.id)?;
            report.stats.metadata_deletes = report.stats.metadata_deletes.saturating_add(1);
            report.stats.row_deletes = report.stats.row_deletes.saturating_add(1);
            report.row_delta = report.row_delta.saturating_sub(1);
            report.changed_ids.push(prepared.id.clone());
        }
        let normalized_deleted = Self::delete_normalized_vector_keys_for_document(
            tx,
            &context.row_schema,
            &prepared.id,
            &context.vector_fields,
        )?;
        check_document_write_failure_point(DocumentWriteFailurePoint::NormalizedVector)?;
        let index_changes = Self::sync_secondary_indexes_for_write(
            tx,
            context,
            &prepared.id,
            existing.payload.as_ref(),
            None,
        )?;
        report.stats.index_deletes = report.stats.index_deletes.saturating_add(
            u64::try_from(normalized_deleted.saturating_add(index_changes.0)).unwrap_or(0),
        );
        report.stats.index_puts = report
            .stats
            .index_puts
            .saturating_add(u64::try_from(index_changes.1).unwrap_or(0));
        Ok(())
    }

    fn sync_secondary_indexes_for_write(
        tx: &mut crate::midge::adapter::AdapterTransaction,
        context: &DocumentWriteBatchContext,
        id: &str,
        previous: Option<&serde_json::Value>,
        next: Option<&serde_json::Value>,
    ) -> Result<(usize, usize), CassieError> {
        let (scalar_deleted, scalar_puts) = Self::sync_scalar_indexes_for_document(
            tx,
            id,
            previous,
            next,
            &context.scalar_indexes,
            &context.row_schema,
        )?;
        let (time_series_deleted, time_series_puts) = Self::sync_time_series_indexes_for_document(
            tx,
            id,
            previous,
            next,
            &context.time_series_indexes,
            context.target_generation,
        )?;
        Ok((
            scalar_deleted.saturating_add(time_series_deleted),
            scalar_puts.saturating_add(time_series_puts),
        ))
    }

    pub(super) fn validate_document(
        schema: &Schema,
        payload: &serde_json::Value,
    ) -> Result<(), CassieError> {
        let map = payload
            .as_object()
            .ok_or_else(|| CassieError::InvalidVector("document must be object".to_string()))?;

        for field in &schema.fields {
            if let Some(value) = map.get(&field.name) {
                if let DataType::Vector(dimensions) = &field.data_type {
                    if value.is_null() && field.nullable {
                        continue;
                    }
                    if let Some(arr) = value.as_array() {
                        if !(1..=16_000).contains(&arr.len())
                            || dimensions.is_some_and(|dim| arr.len() != dim)
                        {
                            return Err(CassieError::InvalidVector(format!(
                                "field '{}' expects {} but got {}",
                                field.name,
                                dimensions.map_or_else(
                                    || "vector".to_string(),
                                    |dim| format!("vector({dim})"),
                                ),
                                arr.len()
                            )));
                        }
                    } else {
                        return Err(CassieError::InvalidVector(format!(
                            "field '{}' expects {} but received non-array",
                            field.name,
                            dimensions.map_or_else(
                                || "vector".to_string(),
                                |dim| format!("vector({dim})"),
                            )
                        )));
                    }
                }
            }
        }
        crate::executor::filter::graph_validation::validate_stored_fields(schema, map)
    }
}

#[cfg(test)]
#[path = "documents/nullable_vectors.rs"]
mod nullable_vectors;
