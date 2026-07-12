use serde::{Deserialize, Serialize};

use super::{collect_scan, CassieError, IndexKind, IndexMeta, Midge, Query, WriteOptions};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
enum IndexPublicationState {
    Prepared,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PendingIndexPublication {
    state: IndexPublicationState,
    index: IndexMeta,
    target_generation: u64,
}

impl Midge {
    pub(crate) fn validate_pending_index_publications(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(Self::index_publication_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
        for (_key, raw) in entries {
            serde_json::from_slice::<PendingIndexPublication>(&raw).map_err(|error| {
                CassieError::Parse(format!("invalid index publication: {error}"))
            })?;
        }
        Ok(())
    }

    pub(super) fn prepare_index_publication(&self, index: &IndexMeta) -> Result<(), CassieError> {
        let pending = PendingIndexPublication {
            state: IndexPublicationState::Prepared,
            index: index.clone(),
            target_generation: self.collection_generation(&index.collection)?,
        };
        let mut tx = self.begin_schema_rw_tx()?;
        tx.put(
            Self::index_publication_key(&index.collection, &index.name),
            serde_json::to_vec(&pending).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }

    /// Rebuilds prepared data-family index state and atomically publishes its schema metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when a pending record cannot be read, rebuilt, or published.
    pub fn replay_pending_index_publications(&self) -> Result<(), CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let entries = collect_scan(
            tx.scan(&Query::new().prefix(Self::index_publication_prefix().into()))
                .map_err(CassieError::from)?,
        )?;
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

    fn publish_prepared_index_locked(
        &self,
        mut publication: PendingIndexPublication,
    ) -> Result<(), CassieError> {
        loop {
            let generation = self.collection_generation(&publication.index.collection)?;
            if generation != publication.target_generation {
                publication.target_generation = generation;
                self.save_pending_index_publication(&publication)?;
            }
            match publication.index.kind {
                IndexKind::Scalar => {
                    self.rebuild_scalar_index_for_index(&publication.index)?;
                }
                IndexKind::TimeSeries => {
                    self.rebuild_time_series_index_for_index(&publication.index)?;
                }
                IndexKind::Column => {
                    self.rebuild_column_batches_for_index(&publication.index)?;
                }
                _ => {}
            }
            if self.collection_generation(&publication.index.collection)?
                != publication.target_generation
            {
                continue;
            }
            let mut tx = self.begin_schema_rw_tx()?;
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
            return tx.commit(WriteOptions::sync()).map_err(CassieError::from);
        }
    }

    fn save_pending_index_publication(
        &self,
        publication: &PendingIndexPublication,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.put(
            Self::index_publication_key(&publication.index.collection, &publication.index.name),
            serde_json::to_vec(publication)
                .map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)
    }
}
