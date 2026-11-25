use serde::de::DeserializeOwned;

use super::{RangeHashRecord, RootHashRecord, RowHashRecord};
use crate::adapter::{AdapterTransaction, CassieError, Midge, ProjectionMeta, Query};
use crate::catalog::relation_database_name;

/// The committed inputs for one projection verification manifest.
pub struct ProjectionManifestSnapshot {
    pub metadata: ProjectionMeta,
    pub root: Option<RootHashRecord>,
    pub ranges: Vec<RangeHashRecord>,
    pub rows: Vec<RowHashRecord>,
}

impl Midge {
    /// Read the descriptor and all requested hashes in one owning-family snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the projection or its committed manifest inputs cannot
    /// be read, or the projection points to an output outside its owning database.
    pub fn projection_manifest_snapshot(
        &self,
        projection: &str,
        include_rows: bool,
    ) -> Result<ProjectionManifestSnapshot, CassieError> {
        let projection = self.canonical_collection_name(projection);
        let tx =
            self.database_tx_for_collection(&projection, cntryl_midge::TransactionMode::ReadOnly)?;
        let metadata = Self::load_projection_metadata_from_tx(&tx, &projection)?
            .ok_or_else(|| CassieError::NotFound(format!("projection not found: {projection}")))?;
        let output = self.canonical_collection_name(
            metadata
                .active_output_collection()
                .unwrap_or(metadata.projection_id()),
        );
        if relation_database_name(&projection) != relation_database_name(&output) {
            return Err(CassieError::Execution(format!(
                "projection '{projection}' has an output outside its owning database"
            )));
        }
        let root = read_root(&tx, &output)?;
        let mut ranges: Vec<RangeHashRecord> =
            read_hashes(&tx, Self::range_hash_prefix(&output), "range")?;
        ranges.sort_by_key(|range| range.range_id);
        let rows = if include_rows {
            let mut rows: Vec<RowHashRecord> =
                read_hashes(&tx, Self::row_hash_prefix(&output), "row")?;
            rows.sort_by(|left, right| left.row_id.cmp(&right.row_id));
            rows
        } else {
            Vec::new()
        };
        Ok(ProjectionManifestSnapshot {
            metadata,
            root,
            ranges,
            rows,
        })
    }
}

fn read_root(tx: &AdapterTransaction, output: &str) -> Result<Option<RootHashRecord>, CassieError> {
    let Some(raw) = tx
        .get(&Midge::root_hash_key(output))
        .map_err(CassieError::from)?
    else {
        return Ok(None);
    };
    let root: RootHashRecord = serde_json::from_slice(&raw)
        .map_err(|error| CassieError::Parse(format!("invalid root hash metadata: {error}")))?;
    let generation = tx
        .get(&Midge::collection_generation_key(output))
        .map_err(CassieError::from)?
        .map_or(Ok(0), |bytes| {
            bytes
                .as_ref()
                .try_into()
                .map(u64::from_be_bytes)
                .map_err(|_| {
                    CassieError::Parse("invalid persisted collection generation".to_owned())
                })
        })?;
    Ok((root.built_generation == generation).then_some(root))
}

fn read_hashes<T: DeserializeOwned>(
    tx: &AdapterTransaction,
    prefix: Vec<u8>,
    kind: &str,
) -> Result<Vec<T>, CassieError> {
    let scan = tx
        .scan(&Query::new().prefix(prefix.into()))
        .map_err(CassieError::from)?;
    scan.map(|entry| {
        let (_, value) = entry.map_err(CassieError::from)?;
        serde_json::from_slice(&value)
            .map_err(|error| CassieError::Parse(format!("invalid {kind} hash metadata: {error}")))
    })
    .collect()
}
