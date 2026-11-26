use serde::de::DeserializeOwned;

use super::{RangeHashRecord, RootHashRecord, RowHashRecord};
use crate::adapter::{AdapterTransaction, CassieError, Midge, ProjectionMeta, Query};
use crate::catalog::{name_matches, parse_name, relation_database_name, ParsedName};

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
        let canonical = self.canonical_collection_name(projection);
        let database =
            relation_database_name(&canonical).unwrap_or_else(|| self.default_database.clone());
        let family = self.database_family(&database)?;
        let tx = self.begin_committed_read_adapter_tx(family.id())?;
        let (tx, database, metadata) =
            if let Some(metadata) = Self::load_projection_metadata_from_tx(&tx, &canonical)? {
                (tx, database, metadata)
            } else {
                self.find_manifest_alias(projection, tx, database)?
            };
        let output = self.canonical_collection_name(
            metadata
                .active_output_collection()
                .unwrap_or(metadata.projection_id()),
        );
        if !crate::catalog::relation_belongs_to_database(&output, &database) {
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

    fn find_manifest_alias(
        &self,
        projection: &str,
        default_tx: AdapterTransaction,
        default_database: String,
    ) -> Result<(AdapterTransaction, String, ProjectionMeta), CassieError> {
        let not_found = || CassieError::NotFound(format!("projection not found: {projection}"));
        if !matches!(
            parse_name(projection),
            Ok(ParsedName::Unqualified(_) | ParsedName::SchemaQualified { .. })
        ) {
            return Err(not_found());
        }

        let mut databases = self.database_names();
        databases.retain(|database| database != &default_database);
        let mut matched = None;
        let mut default_tx = Some(default_tx);
        for database in std::iter::once(default_database).chain(databases) {
            let tx = if let Some(tx) = default_tx.take() {
                tx
            } else {
                let family = self.database_family(&database)?;
                self.begin_committed_read_adapter_tx(family.id())?
            };
            if let Some(metadata) = match_manifest_alias(&tx, projection, matched.is_some())? {
                matched = Some((tx, database, metadata));
            }
        }
        matched.ok_or_else(not_found)
    }
}

fn match_manifest_alias(
    tx: &AdapterTransaction,
    projection: &str,
    already_matched: bool,
) -> Result<Option<ProjectionMeta>, CassieError> {
    let scan = tx
        .scan(&Query::new().prefix(Midge::projection_prefix().into()))
        .map_err(CassieError::from)?;
    let mut matched = None;
    for entry in scan {
        let (_, value) = entry.map_err(CassieError::from)?;
        // Like the existing projection discovery scan, skip unreadable records
        // that cannot establish an alias. Exact lookup validates its descriptor.
        let Ok(metadata) = serde_json::from_slice::<ProjectionMeta>(&value) else {
            continue;
        };
        if name_matches(&metadata.collection, projection) {
            if already_matched || matched.is_some() {
                return Err(CassieError::Execution(format!(
                    "projection name is ambiguous: {projection}"
                )));
            }
            matched = Some(metadata);
        }
    }
    Ok(matched)
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
