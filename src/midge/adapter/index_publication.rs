use serde::{Deserialize, Serialize};

use super::{CassieError, IndexKind, IndexMeta, Midge, Query};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) enum IndexPublicationState {
    Prepared,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(super) struct PendingIndexPublication {
    pub(super) state: IndexPublicationState,
    pub(super) index: IndexMeta,
    pub(super) target_generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) vector_backfill: Option<super::vector_publication::PendingVectorBackfill>,
}

impl Midge {
    pub(super) fn ensure_no_observed_prepared_index_publications(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        let controls = super::PublicationControlScope::current_query_controls();
        for database in self.database_names() {
            if let Some(controls) = controls.as_ref() {
                controls.check_interruption()?;
            }
            let tx =
                self.begin_raw_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?;
            let mut scan = tx
                .scan(&Query::new().prefix(Self::index_publication_prefix().into()))
                .map_err(CassieError::from)?;
            if scan
                .next()
                .transpose()
                .map_err(CassieError::from)?
                .is_some()
            {
                return Err(CassieError::Unsupported(
                    "pending index publication requires sorted recovery before creating another index".to_owned(),
                ));
            }
        }
        let collection = self.canonical_collection_name(collection);
        let database = crate::catalog::relation_database_name(&collection)
            .unwrap_or_else(|| self.default_database.clone());
        let tx = self.begin_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?;
        let mut scan = tx
            .scan(&Query::new().prefix(Self::index_publication_prefix().into()))
            .map_err(CassieError::from)?;
        if scan
            .next()
            .transpose()
            .map_err(CassieError::from)?
            .is_some()
        {
            return Err(CassieError::Unsupported(
                "pending index publication requires sorted recovery before creating another index"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn has_prepared_index_publication(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<bool, CassieError> {
        self.find_prepared_index_publication(collection, name, true)
    }

    pub(super) fn has_persisted_index_publication(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<bool, CassieError> {
        self.find_prepared_index_publication(collection, name, false)
    }

    fn find_prepared_index_publication(
        &self,
        collection: &str,
        name: &str,
        include_overlay: bool,
    ) -> Result<bool, CassieError> {
        let collection = self.canonical_collection_name(collection);
        let database = crate::catalog::relation_database_name(&collection)
            .unwrap_or_else(|| self.default_database.clone());
        let tx = if include_overlay {
            self.begin_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?
        } else {
            self.begin_raw_database_adapter_tx(&database, super::TransactionMode::ReadOnly)?
        };
        let controls = super::PublicationControlScope::current_query_controls();
        let scan = tx
            .scan(&Query::new().prefix(Self::index_publication_prefix().into()))
            .map_err(CassieError::from)?;
        for entry in scan {
            if let Some(controls) = controls.as_ref() {
                controls.check_interruption()?;
            }
            let (_, raw) = entry.map_err(CassieError::from)?;
            let pending = super::schema_cleanup::guard_decode::decode::<PendingIndexPublication>(
                &raw,
                controls.as_ref(),
                "index publication",
            )?;
            if self
                .canonical_collection_name(&pending.value.index.collection)
                .eq_ignore_ascii_case(&collection)
                && (super::schema_cleanup::guard_decode::names_match(
                    &pending.value.index.name,
                    name,
                    controls.as_ref(),
                )? || super::schema_cleanup::guard_decode::names_match(
                    name,
                    &pending.value.index.name,
                    controls.as_ref(),
                )?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn validate_pending_index_publications(&self) -> Result<(), CassieError> {
        let entries = self.scan_database_families(&Self::index_publication_prefix())?;
        for (_key, raw) in entries {
            serde_json::from_slice::<PendingIndexPublication>(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid index publication: {error}"))
            })?;
        }
        Ok(())
    }

    pub(super) fn prepare_index_publication(
        &self,
        index: &IndexMeta,
    ) -> Result<PendingIndexPublication, CassieError> {
        let pending = PendingIndexPublication {
            state: IndexPublicationState::Prepared,
            vector_backfill: None,
            index: index.clone(),
            target_generation: self.collection_generation(&index.collection)?,
        };
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        tx.put(
            Self::index_publication_key(&index.collection, &index.name),
            serde_json::to_vec(&pending).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(pending)
    }

    /// Rebuilds prepared data-family index state and atomically publishes its schema metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when a pending record cannot be read, rebuilt, or published.
    pub fn replay_pending_index_publications(&self) -> Result<(), CassieError> {
        let entries = self.scan_database_families(&Self::index_publication_prefix())?;
        let mut pending = entries
            .into_iter()
            .map(|(_, raw)| {
                serde_json::from_slice::<PendingIndexPublication>(&raw).map_err(|error| {
                    CassieError::Parse(format!("invalid index publication: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        pending.sort_by(|left, right| {
            left.index
                .collection
                .cmp(&right.index.collection)
                .then_with(|| left.index.name.cmp(&right.index.name))
        });
        for publication in pending {
            self.publish_prepared_index(publication)?;
        }
        Ok(())
    }

    fn publish_prepared_index(
        &self,
        publication: PendingIndexPublication,
    ) -> Result<(), CassieError> {
        let collections = vec![publication.index.collection.clone()];
        self.with_collection_write_gates(&collections, || {
            self.publish_prepared_index_locked(publication)
        })
    }

    pub(super) fn publish_prepared_index_locked(
        &self,
        mut publication: PendingIndexPublication,
    ) -> Result<(), CassieError> {
        self.ensure_no_pending_index_cleanup(
            &publication.index.collection,
            &publication.index.name,
        )?;
        if publication.vector_backfill.is_some() {
            self.publish_pending_vector_index(&publication)?;
            return Ok(());
        }
        loop {
            let generation = self.collection_generation(&publication.index.collection)?;
            if generation != publication.target_generation {
                publication.target_generation = generation;
                self.save_pending_index_publication(&publication)?;
            }
            if publication.index.kind == IndexKind::Column
                && self.has_column_batch_maintenance_debt(&publication.index.collection)?
            {
                self.complete_column_batch_maintenance(
                    &publication.index.collection,
                    publication.target_generation,
                    None,
                )?;
            }
            match publication.index.kind {
                IndexKind::Scalar => {
                    self.rebuild_prepared_scalar_index_for_index(&publication.index)?;
                }
                IndexKind::TimeSeries => {
                    self.rebuild_time_series_index_for_index(&publication.index)?;
                }
                IndexKind::Column => {
                    self.rebuild_column_batches_for_index(&publication.index)?;
                }
                IndexKind::FullText => {
                    self.rebuild_prepared_fulltext_index_for_index(&publication.index)?;
                }
                _ => {}
            }
            if self.collection_generation(&publication.index.collection)?
                != publication.target_generation
            {
                continue;
            }
            let mut tx = self.begin_data_rw_tx_for(&publication.index.collection)?;
            tx.put(
                Self::index_key(&publication.index.collection, &publication.index.name),
                serde_json::to_vec(&publication.index)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
            tx.delete(Self::index_publication_key(
                &publication.index.collection,
                &publication.index.name,
            ))
            .map_err(CassieError::from)?;
            return tx
                .commit(self.write_options_sync())
                .map_err(CassieError::from);
        }
    }

    fn save_pending_index_publication(
        &self,
        publication: &PendingIndexPublication,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx_for(&publication.index.collection)?;
        tx.put(
            Self::index_publication_key(&publication.index.collection, &publication.index.name),
            serde_json::to_vec(publication)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)
    }
}
