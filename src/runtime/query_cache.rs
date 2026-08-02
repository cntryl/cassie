use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use cntryl_lexkey::{Encoder, LexKey};
use cntryl_midge::TransactionMode;
use serde::{Deserialize, Serialize};

use crate::app::CassieError;
use crate::executor::filter::SearchContext;
use crate::midge::adapter::{Midge, StorageFamily};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::{stable_fingerprint, PlanCacheKey, RuntimeState};

const TEMP_ROOT: &[u8] = b"cassie";
const TEMP_LEXKEY: &[u8] = b"lexkey";
const TEMP_VERSION: &[u8] = b"v3";
const TEMP_FAMILY: &[u8] = b"t";
const PLAN_ENTRY_FAMILY: &[u8] = b"pe";
const FULLTEXT_STATS_FAMILY: &[u8] = b"fts";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPlanEntry {
    key: PlanCacheKey,
    plan: PhysicalPlan,
    created_at_ms: u64,
}

pub(crate) enum NonDurablePlanOutcome {
    Durable,
    CandidatePending { ttl_seconds: u64 },
    Transient,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FulltextStatsRecord {
    collection: String,
    field: String,
    analyzer_key: String,
    schema_epoch: u64,
    data_epoch: u64,
    created_at_ms: u64,
    context: SearchContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FulltextStatsKey<'a> {
    collection: &'a str,
    field: &'a str,
    analyzer_key: &'a str,
    schema_epoch: u64,
    data_epoch: u64,
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn fingerprint<T: Serialize>(value: &T) -> u64 {
    stable_fingerprint(value)
}

fn plan_entry_key(key: &PlanCacheKey) -> Vec<u8> {
    temp_key(PLAN_ENTRY_FAMILY, fingerprint(key))
}

fn fulltext_stats_key(
    collection: &str,
    field: &str,
    analyzer_key: &str,
    schema_epoch: u64,
    data_epoch: u64,
) -> Vec<u8> {
    let key = FulltextStatsKey {
        collection,
        field,
        analyzer_key,
        schema_epoch,
        data_epoch,
    };
    temp_key(FULLTEXT_STATS_FAMILY, fingerprint(&key))
}

fn temp_key(family: &[u8], fingerprint: u64) -> Vec<u8> {
    let encoded_fingerprint = LexKey::encode_u64(fingerprint);
    let parts = [
        TEMP_ROOT,
        TEMP_LEXKEY,
        TEMP_VERSION,
        TEMP_FAMILY,
        family,
        encoded_fingerprint.as_bytes(),
    ];
    let capacity = parts.iter().map(|part| part.len()).sum::<usize>() + parts.len();
    let mut encoder = Encoder::with_capacity(capacity);
    encoder.encode_composite_into_buf(&parts);
    encoder.into_vec()
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
    tx.put(key, raw, Some(ttl_seconds))
        .map_err(CassieError::from)?;
    tx.commit(midge.write_options_sync())
        .map_err(CassieError::from)?;
    runtime.record_storage_access("temp", true, true);
    Ok(())
}

fn delete_temp_key(midge: &Midge, runtime: &RuntimeState, key: Vec<u8>) -> Result<(), CassieError> {
    let mut tx = midge.temp_tx(TransactionMode::ReadWrite)?;
    tx.delete(key).map_err(CassieError::from)?;
    tx.commit(midge.write_options_sync())
        .map_err(CassieError::from)?;
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

    let storage_key = plan_entry_key(key);
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

    let entry: CachedPlanEntry = if let Ok(entry) = serde_json::from_slice(&raw) {
        entry
    } else {
        runtime.record_query_cache_deserialize_reject();
        runtime.record_query_cache_l2_miss();
        let _ = delete_temp_key(midge, runtime, storage_key);
        return Ok(None);
    };

    if entry.key.schema_epoch != key.schema_epoch {
        runtime.record_query_cache_schema_epoch_reject();
        runtime.record_query_cache_l2_miss();
        return Ok(None);
    }

    runtime.record_query_cache_l2_hit();
    Ok(Some(Arc::new(entry.plan)))
}

pub(crate) fn observe_non_durable_plan_usage(
    midge: &Midge,
    runtime: &RuntimeState,
    key: &PlanCacheKey,
    plan: &Arc<PhysicalPlan>,
    candidate_pending: bool,
) -> Result<NonDurablePlanOutcome, CassieError> {
    if runtime.limits().cf2_plan_ttl_seconds == 0 {
        return Ok(NonDurablePlanOutcome::Transient);
    }

    if candidate_pending {
        let plan_storage_key = plan_entry_key(key);
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
        )?;
        runtime.record_query_cache_promotion();
        return Ok(NonDurablePlanOutcome::Durable);
    }

    let candidate_ttl = runtime.limits().cf2_plan_candidate_ttl_seconds;
    if candidate_ttl == 0 {
        return Ok(NonDurablePlanOutcome::Transient);
    }

    Ok(NonDurablePlanOutcome::CandidatePending {
        ttl_seconds: candidate_ttl,
    })
}

pub(crate) fn lookup_fulltext_stats(
    midge: &Midge,
    runtime: &RuntimeState,
    collection: &str,
    field: &str,
    analyzer_key: &str,
    schema_epoch: u64,
    data_epoch: u64,
) -> Result<Option<SearchContext>, CassieError> {
    if runtime.limits().cf2_fulltext_stats_ttl_seconds == 0 {
        runtime.record_query_cache_fulltext_stats_miss();
        return Ok(None);
    }

    let storage_key = fulltext_stats_key(collection, field, analyzer_key, schema_epoch, data_epoch);
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

    let record: FulltextStatsRecord = if let Ok(record) = serde_json::from_slice(&raw) {
        record
    } else {
        runtime.record_query_cache_deserialize_reject();
        runtime.record_query_cache_fulltext_stats_miss();
        let _ = delete_temp_key(midge, runtime, storage_key);
        return Ok(None);
    };

    if record.schema_epoch != schema_epoch
        || record.data_epoch != data_epoch
        || record.analyzer_key != analyzer_key
    {
        runtime.record_query_cache_schema_epoch_reject();
        runtime.record_query_cache_fulltext_stats_miss();
        return Ok(None);
    }

    runtime.record_query_cache_fulltext_stats_hit();
    Ok(Some(record.context))
}

#[derive(Clone, Copy)]
pub(crate) struct FulltextStatsCacheKey<'a> {
    pub collection: &'a str,
    pub field: &'a str,
    pub analyzer_key: &'a str,
    pub schema_epoch: u64,
    pub data_epoch: u64,
}

pub(crate) fn store_fulltext_stats(
    midge: &Midge,
    runtime: &RuntimeState,
    key: FulltextStatsCacheKey<'_>,
    context: &SearchContext,
) -> Result<(), CassieError> {
    let ttl_seconds = runtime.limits().cf2_fulltext_stats_ttl_seconds;
    if ttl_seconds == 0 {
        return Ok(());
    }

    let record = FulltextStatsRecord {
        collection: key.collection.to_string(),
        field: key.field.to_string(),
        analyzer_key: key.analyzer_key.to_string(),
        schema_epoch: key.schema_epoch,
        data_epoch: key.data_epoch,
        created_at_ms: current_time_millis(),
        context: context.clone(),
    };
    put_temp_json(
        midge,
        runtime,
        fulltext_stats_key(
            key.collection,
            key.field,
            key.analyzer_key,
            key.schema_epoch,
            key.data_epoch,
        ),
        &record,
        ttl_seconds,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CassieRuntimeLimits;
    use crate::planner::logical::LogicalPlan;
    use crate::runtime::{ExecutionMode, ParameterShape};
    use crate::sql::ast::QuerySource;
    use uuid::Uuid;

    fn cache_key(schema_epoch: u64) -> PlanCacheKey {
        PlanCacheKey {
            sql_fingerprint: 42,
            schema_epoch,
            data_epoch: 3,
            index_feedback_epoch: 4,
            cost_model_version: 2,
            adaptive_config_hash: 0,
            parameter_shape: vec![ParameterShape::Int64],
            mode: ExecutionMode::SimpleQuery,
            database: Some("postgres".to_string()),
            search_path: vec!["public".to_string()],
        }
    }

    fn physical_plan() -> PhysicalPlan {
        crate::planner::physical::build(LogicalPlan {
            command: None,
            source: QuerySource::SingleRow,
            collection: String::new(),
            ctes: Vec::new(),
            distinct: false,
            distinct_on: Vec::new(),
            projection: Vec::new(),
            filter: None,
            group_by: Vec::new(),
            having: None,
            order: Vec::new(),
            limit: None,
            offset: None,
            set: None,
        })
    }

    fn cache_fixture(
        label: &str,
        limits: CassieRuntimeLimits,
        test: impl FnOnce(&Midge, &RuntimeState),
    ) {
        let path = std::env::temp_dir().join(format!("cassie-cache-{label}-{}", Uuid::new_v4()));
        let midge = Midge::new_with_data_dir(&path).expect("midge");
        let runtime = RuntimeState::new(limits);
        test(&midge, &runtime);
        drop(midge);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_ignore_malformed_persisted_plan_cache_entries_given_a_matching_cache_key() {
        // Arrange
        cache_fixture(
            "malformed-ignore",
            CassieRuntimeLimits::default(),
            |midge, runtime| {
                let key = cache_key(1);
                midge
                    .raw_put(StorageFamily::Temp, &plan_entry_key(&key), b"not-json")
                    .expect("inject malformed entry");

                // Act
                let result = lookup_plan(midge, runtime, &key).expect("cache lookup");

                // Assert
                assert!(result.is_none());
            },
        );
    }

    #[test]
    fn should_delete_malformed_persisted_plan_cache_entries_after_rejection() {
        // Arrange
        cache_fixture(
            "malformed-delete",
            CassieRuntimeLimits::default(),
            |midge, runtime| {
                let key = cache_key(1);
                let storage_key = plan_entry_key(&key);
                midge
                    .raw_put(StorageFamily::Temp, &storage_key, b"not-json")
                    .expect("inject malformed entry");

                // Act
                lookup_plan(midge, runtime, &key).expect("cache lookup");
                let persisted = midge
                    .raw_get(StorageFamily::Temp, &storage_key)
                    .expect("read rejected entry");

                // Assert
                assert!(persisted.is_none());
            },
        );
    }

    #[test]
    fn should_reject_persisted_plan_cache_entries_given_a_schema_epoch_mismatch() {
        // Arrange
        cache_fixture(
            "schema-epoch",
            CassieRuntimeLimits::default(),
            |midge, runtime| {
                let requested = cache_key(2);
                let entry = CachedPlanEntry {
                    key: cache_key(1),
                    plan: physical_plan(),
                    created_at_ms: current_time_millis(),
                };
                put_temp_json(midge, runtime, plan_entry_key(&requested), &entry, 60)
                    .expect("persist mismatched entry");

                // Act
                let result = lookup_plan(midge, runtime, &requested).expect("cache lookup");

                // Assert
                assert!(result.is_none());
            },
        );
    }

    #[test]
    fn should_invalidate_fulltext_stats_given_a_data_epoch_change() {
        // Arrange
        cache_fixture(
            "fulltext-data-epoch",
            CassieRuntimeLimits::default(),
            |midge, runtime| {
                store_fulltext_stats(
                    midge,
                    runtime,
                    FulltextStatsCacheKey {
                        collection: "documents",
                        field: "body",
                        analyzer_key: "default",
                        schema_epoch: 1,
                        data_epoch: 1,
                    },
                    &SearchContext::default(),
                )
                .expect("store statistics");

                // Act
                let stale =
                    lookup_fulltext_stats(midge, runtime, "documents", "body", "default", 1, 2)
                        .expect("lookup statistics");

                // Assert
                assert!(stale.is_none());
            },
        );
    }

    #[test]
    fn should_fall_back_to_planning_given_a_zero_plan_cache_ttl() {
        // Arrange
        let limits = CassieRuntimeLimits {
            cf2_plan_ttl_seconds: 0,
            ..CassieRuntimeLimits::default()
        };
        cache_fixture("zero-ttl", limits, |midge, runtime| {
            let key = cache_key(1);

            // Act
            let result = lookup_plan(midge, runtime, &key).expect("cache lookup");

            // Assert
            assert!(result.is_none());
        });
    }
}
