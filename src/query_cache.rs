use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cntryl_midge::{TransactionMode, WriteOptions};
use serde::{Deserialize, Serialize};

use crate::app::CassieError;
use crate::executor::filter::SearchContext;
use crate::midge::adapter::{Midge, StorageFamily};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::{PlanCacheKey, RuntimeState};

const PLAN_ENTRY_PREFIX: &str = "__cassie__/cf2/plan/entry/";
const PLAN_CANDIDATE_PREFIX: &str = "__cassie__/cf2/plan/candidate/";
const FULLTEXT_STATS_PREFIX: &str = "__cassie__/cf2/stats/fulltext/";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPlanEntry {
    key: PlanCacheKey,
    plan: PhysicalPlan,
    created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanCandidateEntry {
    key: PlanCacheKey,
    seen_count: u64,
    first_seen_at_ms: u64,
    last_seen_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FulltextStatsRecord {
    collection: String,
    field: String,
    schema_epoch: u64,
    created_at_ms: u64,
    context: SearchContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FulltextStatsKey<'a> {
    collection: &'a str,
    field: &'a str,
    schema_epoch: u64,
}

pub(crate) fn plan_entry_prefix() -> &'static [u8] {
    PLAN_ENTRY_PREFIX.as_bytes()
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn fingerprint<T: Serialize>(value: &T) -> Result<String, CassieError> {
    let bytes = serde_json::to_vec(value).map_err(|error| CassieError::Parse(error.to_string()))?;
    Ok(bytes_to_hex(&bytes))
}

fn plan_entry_key(key: &PlanCacheKey) -> Result<Vec<u8>, CassieError> {
    Ok(format!("{PLAN_ENTRY_PREFIX}{}", fingerprint(key)?).into_bytes())
}

fn plan_candidate_key(key: &PlanCacheKey) -> Result<Vec<u8>, CassieError> {
    Ok(format!("{PLAN_CANDIDATE_PREFIX}{}", fingerprint(key)?).into_bytes())
}

fn fulltext_stats_key(
    collection: &str,
    field: &str,
    schema_epoch: u64,
) -> Result<Vec<u8>, CassieError> {
    let key = FulltextStatsKey {
        collection,
        field,
        schema_epoch,
    };
    Ok(format!("{FULLTEXT_STATS_PREFIX}{}", fingerprint(&key)?).into_bytes())
}

fn put_temp_json<T: Serialize>(
    midge: &Midge,
    runtime: &RuntimeState,
    key: Vec<u8>,
    value: &T,
    ttl_seconds: u64,
) -> Result<(), CassieError> {
    let ttl_seconds = ttl_seconds.max(1);
    let raw = serde_json::to_vec(value).map_err(|error| CassieError::Parse(error.to_string()))?;
    let mut tx = midge.temp_tx(TransactionMode::ReadWrite)?;
    tx.put(key, raw, Some(ttl_seconds)).map_err(CassieError::from)?;
    tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
    runtime.record_storage_access("temp", true, true);
    Ok(())
}

fn delete_temp_key(midge: &Midge, runtime: &RuntimeState, key: Vec<u8>) -> Result<(), CassieError> {
    let mut tx = midge.temp_tx(TransactionMode::ReadWrite)?;
    tx.delete(key).map_err(CassieError::from)?;
    tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
    runtime.record_storage_access("temp", true, true);
    Ok(())
}

pub(crate) fn lookup_plan(
    midge: &Midge,
    runtime: &RuntimeState,
    key: &PlanCacheKey,
) -> Result<Option<Arc<PhysicalPlan>>, CassieError> {
    if runtime.limits().cf2_plan_ttl_seconds == 0 {
        runtime.record_query_cache_l2_miss();
        return Ok(None);
    }

    let storage_key = plan_entry_key(key)?;
    let Some(raw) = (match midge.raw_get(StorageFamily::Temp, &storage_key) {
        Ok(value) => {
            runtime.record_storage_access("temp", false, true);
            value
        }
        Err(error) => {
            runtime.record_storage_access("temp", false, false);
            return Err(error);
        }
    }) else {
        runtime.record_query_cache_l2_miss();
        return Ok(None);
    };

    let entry: CachedPlanEntry = match serde_json::from_slice(&raw) {
        Ok(entry) => entry,
        Err(_) => {
            runtime.record_query_cache_deserialize_reject();
            runtime.record_query_cache_l2_miss();
            let _ = delete_temp_key(midge, runtime, storage_key);
            return Ok(None);
        }
    };

    if entry.key.schema_epoch != key.schema_epoch {
        runtime.record_query_cache_schema_epoch_reject();
        runtime.record_query_cache_l2_miss();
        return Ok(None);
    }

    runtime.record_query_cache_l2_hit();
    Ok(Some(Arc::new(entry.plan)))
}

pub(crate) fn observe_plan_usage(
    midge: &Midge,
    runtime: &RuntimeState,
    key: &PlanCacheKey,
    plan: &Arc<PhysicalPlan>,
    durable_cached: bool,
) -> Result<bool, CassieError> {
    if durable_cached || runtime.limits().cf2_plan_ttl_seconds == 0 {
        return Ok(durable_cached);
    }

    let plan_storage_key = plan_entry_key(key)?;
    if match midge.raw_get(StorageFamily::Temp, &plan_storage_key) {
        Ok(value) => {
            runtime.record_storage_access("temp", false, true);
            value
        }
        Err(error) => {
            runtime.record_storage_access("temp", false, false);
            return Err(error);
        }
    }
    .is_some()
    {
        return Ok(true);
    }

    let candidate_ttl = runtime.limits().cf2_plan_candidate_ttl_seconds;
    if candidate_ttl == 0 {
        return Ok(false);
    }

    let candidate_storage_key = plan_candidate_key(key)?;
    if match midge.raw_get(StorageFamily::Temp, &candidate_storage_key) {
        Ok(value) => {
            runtime.record_storage_access("temp", false, true);
            value
        }
        Err(error) => {
            runtime.record_storage_access("temp", false, false);
            return Err(error);
        }
    }
    .is_some()
    {
        let entry = CachedPlanEntry {
            key: key.clone(),
            plan: (**plan).clone(),
            created_at_ms: current_time_millis(),
        };
        put_temp_json(
            midge,
            runtime,
            plan_storage_key,
            &entry,
            runtime.limits().cf2_plan_ttl_seconds,
        )
        ?;
        delete_temp_key(midge, runtime, candidate_storage_key)?;
        runtime.record_query_cache_promotion();
        return Ok(true);
    }

    let candidate = PlanCandidateEntry {
        key: key.clone(),
        seen_count: 1,
        first_seen_at_ms: current_time_millis(),
        last_seen_at_ms: current_time_millis(),
    };
    put_temp_json(midge, runtime, candidate_storage_key, &candidate, candidate_ttl)?;
    Ok(false)
}

pub(crate) fn lookup_fulltext_stats(
    midge: &Midge,
    runtime: &RuntimeState,
    collection: &str,
    field: &str,
    schema_epoch: u64,
) -> Result<Option<SearchContext>, CassieError> {
    if runtime.limits().cf2_fulltext_stats_ttl_seconds == 0 {
        runtime.record_query_cache_fulltext_stats_miss();
        return Ok(None);
    }

    let storage_key = fulltext_stats_key(collection, field, schema_epoch)?;
    let Some(raw) = (match midge.raw_get(StorageFamily::Temp, &storage_key) {
        Ok(value) => {
            runtime.record_storage_access("temp", false, true);
            value
        }
        Err(error) => {
            runtime.record_storage_access("temp", false, false);
            return Err(error);
        }
    }) else {
        runtime.record_query_cache_fulltext_stats_miss();
        return Ok(None);
    };

    let record: FulltextStatsRecord = match serde_json::from_slice(&raw) {
        Ok(record) => record,
        Err(_) => {
            runtime.record_query_cache_deserialize_reject();
            runtime.record_query_cache_fulltext_stats_miss();
            let _ = delete_temp_key(midge, runtime, storage_key);
            return Ok(None);
        }
    };

    if record.schema_epoch != schema_epoch {
        runtime.record_query_cache_schema_epoch_reject();
        runtime.record_query_cache_fulltext_stats_miss();
        return Ok(None);
    }

    runtime.record_query_cache_fulltext_stats_hit();
    Ok(Some(record.context))
}

pub(crate) fn store_fulltext_stats(
    midge: &Midge,
    runtime: &RuntimeState,
    collection: &str,
    field: &str,
    schema_epoch: u64,
    context: &SearchContext,
) -> Result<(), CassieError> {
    let ttl_seconds = runtime.limits().cf2_fulltext_stats_ttl_seconds;
    if ttl_seconds == 0 {
        return Ok(());
    }

    let record = FulltextStatsRecord {
        collection: collection.to_string(),
        field: field.to_string(),
        schema_epoch,
        created_at_ms: current_time_millis(),
        context: context.clone(),
    };
    put_temp_json(
        midge,
        runtime,
        fulltext_stats_key(collection, field, schema_epoch)?,
        &record,
        ttl_seconds,
    )
}
