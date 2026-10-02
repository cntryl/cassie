use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::documents::DocumentWriteBatchReport;
use super::index_publication::{IndexPublicationState, PendingIndexPublication};
use super::{
    collect_scan, key_encoding, CassieError, DocumentWriteBatchOptions, DocumentWriteOp, IndexKind,
    IndexMeta, Midge, Query, VectorIndexRecord,
};

const STAGING_BATCH_SIZE: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct PendingVectorBackfill {
    version: u32,
    publication_id: String,
    vector_index: VectorIndexRecord,
    row_count: usize,
    row_digest: Vec<u8>,
    source_generation: u64,
    publish_sql_index: bool,
    record_rollup_maintenance_debt: bool,
    record_materialized_projection_maintenance_debt: bool,
}

#[derive(Serialize, Deserialize)]
struct BackfillRow {
    id: String,
    payload: serde_json::Value,
}

impl Midge {
    pub(crate) fn publish_vector_index_with_backfill(
        &self,
        index: &VectorIndexRecord,
        sql_index: Option<&IndexMeta>,
        rows: Vec<(String, serde_json::Value)>,
        options: &DocumentWriteBatchOptions,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        let mut index = index.clone();
        index.collection = self.canonical_collection_name(&index.collection);
        self.ensure_no_pending_vector_publication(&index.collection)?;
        let publication_id = uuid::Uuid::new_v4().to_string();
        let metadata = sql_index.map_or_else(
            || {
                Ok(IndexMeta {
                    collection: index.collection.clone(),
                    name: format!("__rest_vector_{publication_id}"),
                    field: index.field.clone(),
                    fields: vec![index.field.clone()],
                    expressions: Vec::new(),
                    include_fields: Vec::new(),
                    predicate: None,
                    kind: IndexKind::Vector,
                    unique: false,
                    options: BTreeMap::new(),
                })
            },
            |metadata| self.prepare_index_metadata(metadata),
        )?;
        let rows = rows
            .into_iter()
            .map(|(id, payload)| BackfillRow { id, payload })
            .collect::<Vec<_>>();
        let source_generation = self.collection_generation(&index.collection)?;
        let mut digest = Sha256::new();
        for (batch, chunk) in rows.chunks(STAGING_BATCH_SIZE).enumerate() {
            let mut tx = self.begin_schema_rw_tx()?;
            for (offset, row) in chunk.iter().enumerate() {
                let ordinal =
                    u64::try_from(batch * STAGING_BATCH_SIZE + offset).map_err(parse_error)?;
                let value = serde_json::to_vec(row).map_err(parse_error)?;
                digest.update(
                    u64::try_from(value.len())
                        .map_err(parse_error)?
                        .to_be_bytes(),
                );
                digest.update(&value);
                tx.put(
                    key_encoding::vector_backfill_row_key(&publication_id, ordinal),
                    value,
                    None,
                )
                .map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
        let publication = PendingIndexPublication {
            state: IndexPublicationState::Prepared,
            index: metadata,
            target_generation: source_generation,
            vector_backfill: Some(PendingVectorBackfill {
                version: 1,
                publication_id,
                vector_index: index,
                row_count: rows.len(),
                row_digest: digest.finalize().to_vec(),
                source_generation,
                publish_sql_index: sql_index.is_some(),
                record_rollup_maintenance_debt: options.record_rollup_maintenance_debt,
                record_materialized_projection_maintenance_debt: options
                    .record_materialized_projection_maintenance_debt,
            }),
        };
        let mut tx = self.begin_schema_rw_tx()?;
        tx.put(
            Self::index_publication_key(&publication.index.collection, &publication.index.name),
            serde_json::to_vec(&publication).map_err(parse_error)?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        super::check_index_publication_failure_point()
            .and_then(|()| self.publish_pending_vector_index(&publication))
            .map_err(|error| {
            CassieError::Storage(format!("recoverable vector index publication for '{}': {error}; preserve the journal and restart before further writes", publication.index.collection))
        })
    }

    fn ensure_no_pending_vector_publication(&self, collection: &str) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        for (_, raw) in collect_scan(
            tx.scan(&Query::new().prefix(Self::index_publication_prefix().into()))
                .map_err(CassieError::from)?,
        )? {
            let pending: PendingIndexPublication =
                serde_json::from_slice(&raw).map_err(parse_error)?;
            if pending.index.collection == collection && pending.vector_backfill.is_some() {
                return Err(CassieError::Storage(format!("pending vector publication for '{collection}' requires recovery before another index build")));
            }
        }
        Ok(())
    }

    pub(super) fn publish_pending_vector_index(
        &self,
        publication: &PendingIndexPublication,
    ) -> Result<DocumentWriteBatchReport, CassieError> {
        let pending = publication
            .vector_backfill
            .as_ref()
            .ok_or_else(|| CassieError::Parse("missing vector backfill publication".to_string()))?;
        if pending.version != 1
            || pending.vector_index.collection != publication.index.collection
            || publication.index.kind != IndexKind::Vector
            || pending.vector_index.field != publication.index.field
            || pending.source_generation != publication.target_generation
            || uuid::Uuid::parse_str(&pending.publication_id).is_err()
        {
            return Err(CassieError::Parse(
                "invalid vector backfill publication version or identity".to_string(),
            ));
        }
        let rows = self.load_vector_backfill_rows(pending)?;
        let marker = key_encoding::vector_backfill_applied_key(&pending.publication_id);
        let tx = self.begin_data_readonly_tx_for(&pending.vector_index.collection)?;
        let applied = match tx.get(&marker).map_err(CassieError::from)? {
            Some(value) if value == pending.publication_id.as_bytes() => true,
            Some(_) => {
                return Err(CassieError::Parse(
                    "invalid vector backfill applied marker".to_string(),
                ))
            }
            None => false,
        };
        drop(tx);
        let expected_generation = if applied {
            pending.source_generation.checked_add(1).ok_or_else(|| {
                CassieError::Storage("vector backfill generation overflow".to_string())
            })?
        } else {
            pending.source_generation
        };
        if self.collection_generation(&pending.vector_index.collection)? != expected_generation {
            return Err(CassieError::Storage(
                "vector backfill generation changed; refusing to overwrite newer data".to_string(),
            ));
        }
        let mut report = DocumentWriteBatchReport::default();
        if applied {
            let schema = self.row_schema(&pending.vector_index.collection)?;
            for row in &rows {
                let stored = self
                    .get_document(&pending.vector_index.collection, &row.id)?
                    .ok_or_else(|| {
                        CassieError::Storage(
                            "vector backfill row missing; refusing publication".to_string(),
                        )
                    })?;
                if super::encode_row(&schema, &stored.payload)?
                    != super::encode_row(&schema, &row.payload)?
                {
                    return Err(CassieError::Storage(
                        "vector backfill payload changed; refusing publication".to_string(),
                    ));
                }
            }
        } else if !rows.is_empty() {
            let mut options = DocumentWriteBatchOptions::sync(self.write_options_sync());
            options.publication_marker =
                Some((marker.clone(), pending.publication_id.as_bytes().to_vec()));
            options.record_rollup_maintenance_debt = pending.record_rollup_maintenance_debt;
            options.record_materialized_projection_maintenance_debt =
                pending.record_materialized_projection_maintenance_debt;
            report = self.apply_document_write_batch_with_options(
                &pending.vector_index.collection,
                rows.into_iter()
                    .map(|row| DocumentWriteOp::Put {
                        id: row.id,
                        payload: row.payload,
                    })
                    .collect(),
                &options,
            )?;
        }
        let index = self.prepare_vector_index_sidecars(pending.vector_index.clone())?;
        self.commit_vector_publication_metadata(publication, &index)?;
        let _ = self.delete_vector_backfill_rows(&pending.publication_id);
        let _ = self.delete_vector_backfill_marker(&index.collection, marker);
        Ok(report)
    }

    fn commit_vector_publication_metadata(
        &self,
        publication: &PendingIndexPublication,
        index: &VectorIndexRecord,
    ) -> Result<(), CassieError> {
        let pending = publication
            .vector_backfill
            .as_ref()
            .ok_or_else(|| CassieError::Parse("missing vector backfill publication".to_string()))?;
        let mut tx = self.begin_schema_rw_tx()?;
        tx.put(
            Self::vector_index_key(&index.collection, &index.field),
            serde_json::to_vec(&index).map_err(parse_error)?,
            None,
        )
        .map_err(CassieError::from)?;
        if pending.publish_sql_index {
            tx.put(
                Self::index_key(&publication.index.collection, &publication.index.name),
                serde_json::to_vec(&publication.index).map_err(parse_error)?,
                None,
            )
            .map_err(CassieError::from)?;
        }
        tx.delete(Self::index_publication_key(
            &publication.index.collection,
            &publication.index.name,
        ))
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    fn delete_vector_backfill_marker(
        &self,
        collection: &str,
        marker: Vec<u8>,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(collection)?;
        tx.delete(marker).map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }

    fn load_vector_backfill_rows(
        &self,
        pending: &PendingVectorBackfill,
    ) -> Result<Vec<BackfillRow>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(
                key_encoding::vector_backfill_row_prefix(Some(&pending.publication_id)).into(),
            ))
            .map_err(CassieError::from)?,
        )?;
        if entries.len() != pending.row_count {
            return Err(CassieError::Parse(
                "vector backfill staged row count mismatch".to_string(),
            ));
        }
        let mut digest = Sha256::new();
        let mut ids = HashSet::new();
        let mut rows = Vec::with_capacity(entries.len());
        for (ordinal, (key, raw)) in entries.into_iter().enumerate() {
            if key
                != key_encoding::vector_backfill_row_key(
                    &pending.publication_id,
                    u64::try_from(ordinal).map_err(parse_error)?,
                )
            {
                return Err(CassieError::Parse(
                    "vector backfill staged ordinal mismatch".to_string(),
                ));
            }
            digest.update(u64::try_from(raw.len()).map_err(parse_error)?.to_be_bytes());
            digest.update(&raw);
            let row: BackfillRow = serde_json::from_slice(&raw).map_err(parse_error)?;
            if !ids.insert(row.id.clone()) {
                return Err(CassieError::Parse(
                    "duplicate vector backfill row identity".to_string(),
                ));
            }
            rows.push(row);
        }
        if digest.finalize().as_slice() != pending.row_digest {
            return Err(CassieError::Parse(
                "vector backfill staged digest mismatch".to_string(),
            ));
        }
        Ok(rows)
    }

    fn delete_vector_backfill_rows(&self, publication_id: &str) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let rows = collect_scan(
            tx.scan(
                &Query::new()
                    .prefix(key_encoding::vector_backfill_row_prefix(Some(publication_id)).into()),
            )
            .map_err(CassieError::from)?,
        )?;
        drop(tx);
        for chunk in rows.chunks(STAGING_BATCH_SIZE) {
            let mut tx = self.begin_schema_rw_tx()?;
            for (key, _) in chunk {
                tx.delete(key.clone()).map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
        Ok(())
    }

    pub(crate) fn cleanup_vector_publication_staging(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let pending = collect_scan(
            tx.scan(&Query::new().prefix(Self::index_publication_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        let ids = pending
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingIndexPublication>(&raw).map_err(parse_error)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|record| record.vector_backfill.map(|vector| vector.publication_id))
            .collect::<Vec<_>>();
        let rows = collect_scan(
            tx.scan(&Query::new().prefix(key_encoding::vector_backfill_row_prefix(None).into()))
                .map_err(CassieError::from)?,
        )?;
        drop(tx);
        let prefixes = ids
            .iter()
            .map(|id| key_encoding::vector_backfill_row_prefix(Some(id)))
            .collect::<Vec<_>>();
        let orphaned = rows
            .into_iter()
            .filter(|(key, _)| !prefixes.iter().any(|prefix| key.starts_with(prefix)))
            .collect::<Vec<_>>();
        for chunk in orphaned.chunks(STAGING_BATCH_SIZE) {
            let mut tx = self.begin_schema_rw_tx()?;
            for (key, _) in chunk {
                tx.delete(key.clone()).map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
        let active = ids
            .iter()
            .map(|id| key_encoding::vector_backfill_applied_key(id))
            .collect::<HashSet<_>>();
        for database in self.list_databases()? {
            let tx = self.database_tx(&database.name, cntryl_midge::TransactionMode::ReadOnly)?;
            let markers = collect_scan(
                tx.scan(
                    &Query::new().prefix(key_encoding::vector_backfill_applied_prefix().into()),
                )
                .map_err(CassieError::from)?,
            )?;
            drop(tx);
            for chunk in markers.chunks(STAGING_BATCH_SIZE) {
                let mut tx =
                    self.database_tx(&database.name, cntryl_midge::TransactionMode::ReadWrite)?;
                for (key, _) in chunk {
                    if !active.contains(key) {
                        tx.delete(key.clone()).map_err(CassieError::from)?;
                    }
                }
                tx.commit(self.write_options_sync())
                    .map_err(CassieError::from)?;
            }
        }
        Ok(())
    }
}

fn parse_error(error: impl std::fmt::Display) -> CassieError {
    CassieError::Parse(error.to_string())
}
