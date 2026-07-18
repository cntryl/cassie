use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;

use cntryl_midge::Query;

use super::{
    decode_projected_row, decode_row, key_encoding, metadata, time_series_scan_query,
    BucketIdentity, CassieError, DocumentRef, IndexMeta, Midge, TimeSeriesIndexScanHit,
    CORRUPT_BUCKET_METADATA, DANGLING_BUCKET_MEMBERSHIP, INCOMPLETE_BUCKET_MEMBERSHIP,
    MISSING_BUCKET_METADATA, STALE_BUCKET_METADATA,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

const CONTROLLED_PAGE_ENTRIES: usize = 128;

#[derive(Debug)]
pub(crate) struct ControlledTimeSeriesIndexScanReport {
    pub hits: Vec<TimeSeriesIndexScanHit>,
    pub entries_scanned: usize,
    pub generation: u64,
    pub memory: Vec<QueryMemoryReservation>,
}

#[derive(Debug)]
pub(crate) enum ControlledTimeSeriesIndexScanOutcome {
    Native(ControlledTimeSeriesIndexScanReport),
    Fallback(&'static str),
}

#[derive(Debug)]
pub(crate) struct ControlledTimeSeriesDocumentScanReport {
    pub documents: Vec<DocumentRef>,
    pub memory: QueryMemoryReservation,
}

#[derive(Debug)]
pub(crate) enum ControlledTimeSeriesDocumentScanOutcome {
    Native(ControlledTimeSeriesDocumentScanReport),
    Fallback(&'static str),
}

struct ControlledIntegrity {
    generation: u64,
    manifest_total: u64,
    expected_total: u64,
    expected_counts: BTreeMap<BucketIdentity, u64>,
    memory: QueryMemoryReservation,
}

struct ControlledIndexRequest<'a> {
    index: &'a IndexMeta,
    relation_id: u64,
    index_id: u64,
    partition_key: Option<&'a str>,
    lower_bucket_seconds: Option<i64>,
    upper_bucket_seconds: Option<i64>,
}

struct ControlledHits {
    hits: Vec<TimeSeriesIndexScanHit>,
    observed_counts: BTreeMap<BucketIdentity, u64>,
    memory: QueryMemoryReservation,
}

enum ControlledIntegrityOutcome {
    Valid(ControlledIntegrity),
    Fallback(&'static str),
}

impl Midge {
    pub(crate) fn scan_time_series_index_controlled(
        &self,
        index: &IndexMeta,
        partition_key: Option<&str>,
        lower_bucket_seconds: Option<i64>,
        upper_bucket_seconds: Option<i64>,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledTimeSeriesIndexScanOutcome, CassieError> {
        let index = self.resolved_time_series_index(index)?;
        if !Self::time_series_index_supports_storage(&index) {
            return Err(CassieError::Unsupported(format!(
                "time-series index '{}' has unsupported bucket storage options",
                index.name
            )));
        }
        let tx = self.begin_data_readonly_tx_for(&index.collection)?;
        let (relation_id, index_id) = Self::time_series_storage_ids(&index)?;
        let integrity = match self.load_controlled_integrity(
            &tx,
            &index.collection,
            relation_id,
            index_id,
            controls,
        )? {
            ControlledIntegrityOutcome::Valid(integrity) => integrity,
            ControlledIntegrityOutcome::Fallback(reason) => {
                return Ok(ControlledTimeSeriesIndexScanOutcome::Fallback(reason));
            }
        };
        let request = ControlledIndexRequest {
            index: &index,
            relation_id,
            index_id,
            partition_key,
            lower_bucket_seconds,
            upper_bucket_seconds,
        };
        let decoded = load_controlled_hits(self, &tx, &request, controls)?;
        if !requested_counts_match(
            &integrity,
            &decoded.observed_counts,
            partition_key,
            lower_bucket_seconds,
            upper_bucket_seconds,
        ) {
            return Ok(ControlledTimeSeriesIndexScanOutcome::Fallback(
                INCOMPLETE_BUCKET_MEMBERSHIP,
            ));
        }
        if decoded
            .observed_counts
            .keys()
            .any(|bucket| !integrity.expected_counts.contains_key(bucket))
        {
            return Ok(ControlledTimeSeriesIndexScanOutcome::Fallback(
                MISSING_BUCKET_METADATA,
            ));
        }
        if integrity.expected_total != integrity.manifest_total {
            return Ok(ControlledTimeSeriesIndexScanOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        }
        let entries_scanned = decoded.hits.len();
        Ok(ControlledTimeSeriesIndexScanOutcome::Native(
            ControlledTimeSeriesIndexScanReport {
                hits: decoded.hits,
                entries_scanned,
                generation: integrity.generation,
                memory: vec![integrity.memory, decoded.memory],
            },
        ))
    }

    fn load_controlled_integrity(
        &self,
        tx: &cntryl_midge::Transaction,
        collection: &str,
        relation_id: u64,
        index_id: u64,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledIntegrityOutcome, CassieError> {
        check_controls(controls)?;
        let mut memory = controls.reserve_query_memory(0)?;
        let manifest_key = key_encoding::time_series_index_manifest_key(relation_id, index_id);
        let manifest_raw = tx.get(&manifest_key).map_err(CassieError::from)?;
        check_controlled_entry(self, controls)?;
        let Some(manifest_raw) = manifest_raw else {
            return Ok(ControlledIntegrityOutcome::Fallback(
                MISSING_BUCKET_METADATA,
            ));
        };
        memory.try_grow(manifest_key.len().saturating_add(manifest_raw.len()))?;
        let Ok(manifest) = metadata::decode_manifest(&manifest_raw) else {
            return Ok(ControlledIntegrityOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        };
        if manifest.version != metadata::FORMAT_VERSION {
            return Ok(ControlledIntegrityOutcome::Fallback(
                CORRUPT_BUCKET_METADATA,
            ));
        }
        let generation = self.collection_generation(collection)?;
        if manifest.generation != generation {
            return Ok(ControlledIntegrityOutcome::Fallback(STALE_BUCKET_METADATA));
        }
        let count_prefix =
            key_encoding::time_series_index_bucket_count_prefix(relation_id, index_id);
        let mut expected_counts = BTreeMap::new();
        let mut expected_total = 0u64;
        controlled_scan_pages(
            self,
            tx,
            controls,
            |start_key| {
                let mut query = Query::new()
                    .prefix(count_prefix.clone().into())
                    .limit(CONTROLLED_PAGE_ENTRIES);
                if let Some(start_key) = start_key {
                    query = query.start_key(start_key.into());
                }
                query
            },
            |key, raw| {
                memory.try_grow(
                    key.len()
                        .saturating_add(raw.len())
                        .saturating_add(size_of::<BucketIdentity>()),
                )?;
                let Some((partition, start_seconds)) =
                    key_encoding::decode_time_series_bucket_count_key(key, &count_prefix)
                else {
                    return Err(CassieError::Parse(
                        "invalid time-series bucket count key".to_string(),
                    ));
                };
                let count = metadata::decode_count(raw)?;
                if count == 0
                    || expected_counts
                        .insert(
                            BucketIdentity {
                                partition,
                                start_seconds,
                            },
                            count,
                        )
                        .is_some()
                {
                    return Err(CassieError::Parse(
                        "corrupt time-series bucket counts".to_string(),
                    ));
                }
                expected_total = expected_total.checked_add(count).ok_or_else(|| {
                    CassieError::Parse("time-series membership count overflow".to_string())
                })?;
                Ok(())
            },
        )?;
        Ok(ControlledIntegrityOutcome::Valid(ControlledIntegrity {
            generation,
            manifest_total: manifest.total_membership,
            expected_total,
            expected_counts,
            memory,
        }))
    }

    pub(crate) fn scan_time_series_hit_documents_controlled(
        &self,
        index: &IndexMeta,
        hits: &[TimeSeriesIndexScanHit],
        fields: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<ControlledTimeSeriesDocumentScanOutcome, CassieError> {
        let index = self.resolved_time_series_index(index)?;
        let collection = &index.collection;
        let row_schema = self.row_schema(collection)?;
        let projection = fields
            .iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let tx = self.begin_data_readonly_tx_for(collection)?;
        let mut memory = controls.reserve_query_memory(0)?;
        let mut documents = Vec::new();
        for hit in hits {
            check_controls(controls)?;
            let raw = match tx
                .get(&Self::row_key(row_schema.relation_id, &hit.id))
                .map_err(CassieError::from)?
            {
                Some(raw) => Some(raw),
                None => tx
                    .get(&Self::doc_key(collection, &hit.id))
                    .map_err(CassieError::from)?,
            };
            check_controlled_entry(self, controls)?;
            let Some(raw) = raw else {
                return Ok(ControlledTimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            };
            memory.try_grow(
                raw.len()
                    .saturating_mul(3)
                    .saturating_add(hit.id.len())
                    .saturating_add(size_of::<DocumentRef>()),
            )?;
            let full_payload = decode_row(&row_schema, &raw)?;
            let Some(expected_entry) =
                Self::time_series_index_entry(&index, &hit.id, &full_payload)?
            else {
                return Ok(ControlledTimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            };
            if expected_entry.key != hit.entry_key {
                return Ok(ControlledTimeSeriesDocumentScanOutcome::Fallback(
                    DANGLING_BUCKET_MEMBERSHIP,
                ));
            }
            let payload = decode_projected_row(&row_schema, &raw, &projection)?;
            documents.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!(
                    "unable to retain controlled time-series row: {error}"
                ))
            })?;
            documents.push(DocumentRef {
                id: hit.id.clone(),
                payload,
            });
        }
        Ok(ControlledTimeSeriesDocumentScanOutcome::Native(
            ControlledTimeSeriesDocumentScanReport { documents, memory },
        ))
    }
}

fn requested_counts_match(
    integrity: &ControlledIntegrity,
    observed_counts: &BTreeMap<BucketIdentity, u64>,
    partition_key: Option<&str>,
    lower_bucket_seconds: Option<i64>,
    upper_bucket_seconds: Option<i64>,
) -> bool {
    for (bucket, expected) in integrity.expected_counts.iter().filter(|(bucket, _)| {
        partition_key.is_none_or(|partition| partition == bucket.partition)
            && lower_bucket_seconds.is_none_or(|lower| bucket.start_seconds >= lower)
            && upper_bucket_seconds.is_none_or(|upper| bucket.start_seconds < upper)
    }) {
        if observed_counts.get(bucket).copied().unwrap_or_default() != *expected {
            return false;
        }
    }
    true
}

fn load_controlled_hits(
    midge: &Midge,
    tx: &cntryl_midge::Transaction,
    request: &ControlledIndexRequest<'_>,
    controls: &QueryExecutionControls,
) -> Result<ControlledHits, CassieError> {
    let data_prefix = Midge::time_series_index_data_prefix(request.relation_id, request.index_id);
    let mut memory = controls.reserve_query_memory(0)?;
    let mut seen_ids = BTreeSet::new();
    let mut observed_counts = BTreeMap::<BucketIdentity, u64>::new();
    let mut hits = Vec::new();
    controlled_scan_pages(
        midge,
        tx,
        controls,
        |start_key| controlled_hit_query(request, start_key),
        |key, raw| {
            let retained = key
                .len()
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(size_of::<TimeSeriesIndexScanHit>()))
                .ok_or_else(|| {
                    CassieError::ResourceLimit("time-series hit size overflow".to_string())
                })?;
            memory.try_grow(retained)?;
            let decoded = super::decode_time_series_hits(
                vec![(key.to_vec(), raw.to_vec())],
                &data_prefix,
                &request.index.name,
            )?;
            let hit =
                decoded.hits.into_iter().next().ok_or_else(|| {
                    CassieError::Parse("missing decoded time-series hit".to_string())
                })?;
            if !seen_ids.insert(hit.id.clone()) {
                return Err(CassieError::Parse(
                    "duplicate time-series membership id".to_string(),
                ));
            }
            for (bucket, count) in decoded.observed_counts {
                let observed = observed_counts.entry(bucket).or_default();
                *observed = observed.saturating_add(count);
            }
            hits.try_reserve_exact(1).map_err(|error| {
                CassieError::ResourceLimit(format!(
                    "unable to retain controlled time-series hit: {error}"
                ))
            })?;
            hits.push(hit);
            Ok(())
        },
    )?;
    Ok(ControlledHits {
        hits,
        observed_counts,
        memory,
    })
}

fn controlled_hit_query(request: &ControlledIndexRequest<'_>, start_key: Option<Vec<u8>>) -> Query {
    let mut query = time_series_scan_query(
        request.relation_id,
        request.index_id,
        request.partition_key,
        request.lower_bucket_seconds,
        request.upper_bucket_seconds,
    )
    .limit(CONTROLLED_PAGE_ENTRIES);
    if let Some(start_key) = start_key {
        query = query.start_key(start_key.into());
    }
    query
}

fn controlled_scan_pages(
    midge: &Midge,
    tx: &cntryl_midge::Transaction,
    controls: &QueryExecutionControls,
    mut query_for_page: impl FnMut(Option<Vec<u8>>) -> Query,
    mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    let mut next_key = None;
    loop {
        check_controls(controls)?;
        let query = query_for_page(next_key.take());
        let scan = tx.scan(&query).map_err(CassieError::from)?;
        let mut entries = 0usize;
        let mut last_key = None;
        for entry in scan {
            check_controls(controls)?;
            let (key, value) = entry.map_err(CassieError::from)?;
            check_controlled_entry(midge, controls)?;
            entries = entries.saturating_add(1);
            last_key = Some(key.to_vec());
            visit(&key, &value)?;
        }
        if entries < CONTROLLED_PAGE_ENTRIES {
            return Ok(());
        }
        let mut start = last_key.ok_or_else(|| {
            CassieError::Execution("controlled time-series page lost its cursor".to_string())
        })?;
        start.push(0);
        next_key = Some(start);
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

fn check_controlled_entry(
    midge: &Midge,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    check_controls(controls)?;
    midge.record_query_scan_entry();
    if super::super::query_scan_control::should_cancel_controlled_query_scan() {
        return Err(CassieError::QueryCancelled);
    }
    Ok(())
}
