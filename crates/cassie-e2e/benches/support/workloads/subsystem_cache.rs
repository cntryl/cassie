//! Production cache benchmark fixtures and their managed setup payloads.
use std::sync::Arc;
use std::time::Instant;

use cassie::config::{CassieRuntimeLimits, ExecutionResultCacheEnabled};
use cassie::executor::QueryResult;
use cassie::runtime::{ExecutionMode, ExecutionResultCacheKey, PlanCacheKey, RuntimeState};
use cassie::types::Value;

use super::{assert_tier2_fixture, usize_to_u64, PhysicalPlanningFixture};

/// Direct production plan-cache and execution-result-cache operations.
pub struct CacheFixture {
    plan_runtime: Arc<RuntimeState>,
    result_runtime: Option<Arc<RuntimeState>>,
    plan_hit_key: PlanCacheKey,
    plan_miss_key: PlanCacheKey,
    result_hit_key: ExecutionResultCacheKey,
}

impl CacheFixture {
    #[must_use]
    pub fn new(entries: usize, include_result_cache: bool) -> Self {
        assert_tier2_fixture(entries);
        let planning = PhysicalPlanningFixture::new(1);
        let plan = Arc::new(planning.sample_physical_plan());
        let plan_limits = CassieRuntimeLimits {
            plan_cache_entries: entries,
            execution_result_cache_enabled: ExecutionResultCacheEnabled::disabled(),
            ..CassieRuntimeLimits::default()
        };
        let plan_runtime = Arc::new(RuntimeState::new(plan_limits));
        for fingerprint in 0..entries {
            plan_runtime.plan_cache_store(plan_key(usize_to_u64(fingerprint)), plan.clone(), false);
        }
        let plan_hit_key = plan_key(usize_to_u64(entries.saturating_sub(1)));
        let plan_miss_key = plan_key(usize_to_u64(entries));

        let result_hit_key = result_key(usize_to_u64(entries.saturating_sub(1)));
        let result_runtime = include_result_cache.then(|| {
            let result_limits = CassieRuntimeLimits {
                execution_result_cache_enabled: ExecutionResultCacheEnabled::enabled(),
                execution_result_cache_max_entries: entries,
                execution_result_cache_max_bytes: 64 * 1024 * 1024,
                ..CassieRuntimeLimits::default()
            };
            let runtime = Arc::new(RuntimeState::new(result_limits));
            for fingerprint in 0..entries {
                let key = result_key(usize_to_u64(fingerprint));
                runtime
                    .execution_result_cache_store(&key, sample_query_result(fingerprint, &runtime));
            }
            runtime
        });

        Self {
            plan_runtime,
            result_runtime,
            plan_hit_key,
            plan_miss_key,
            result_hit_key,
        }
    }

    #[must_use]
    pub fn plan_hit(&self) -> usize {
        let plan = self
            .plan_runtime
            .plan_cache_lookup(&self.plan_hit_key)
            .expect("benchmark plan cache should hit");
        std::hint::black_box(plan);
        1
    }

    #[must_use]
    pub fn plan_miss(&self) -> usize {
        assert!(
            self.plan_runtime
                .plan_cache_lookup(&self.plan_miss_key)
                .is_none(),
            "benchmark plan cache should miss"
        );
        1
    }

    #[must_use]
    pub fn result_hit(&self) -> usize {
        self.result_runtime
            .as_ref()
            .expect("result cache fixture should be enabled")
            .execution_result_cache_lookup(&self.result_hit_key)
            .expect("benchmark result cache should hit")
            .row_count()
    }

    #[must_use]
    pub fn plan_runtime(&self) -> Arc<RuntimeState> {
        self.plan_runtime.clone()
    }

    #[must_use]
    pub fn result_runtime(&self) -> Arc<RuntimeState> {
        self.result_runtime
            .as_ref()
            .expect("result cache fixture should be enabled")
            .clone()
    }
}

fn plan_key(sql_fingerprint: u64) -> PlanCacheKey {
    PlanCacheKey {
        sql_fingerprint,
        schema_epoch: 1,
        data_epoch: 1,
        index_feedback_epoch: 0,
        cost_model_version: 1,
        adaptive_config_hash: 0,
        parameter_shape: Vec::new(),
        mode: ExecutionMode::SimpleQuery,
        database: None,
        search_path: Vec::new(),
    }
}

fn result_key(sql_fingerprint: u64) -> ExecutionResultCacheKey {
    ExecutionResultCacheKey {
        sql_fingerprint,
        params_hash: 0,
        schema_epoch: 1,
        data_epoch: 1,
        user: "benchmark".to_string(),
        database: None,
        search_path: Vec::new(),
        settings_fingerprint: 0,
        mode: ExecutionMode::SimpleQuery,
    }
}

fn sample_query_result(index: usize, runtime: &RuntimeState) -> QueryResult {
    let controls = runtime.query_controls(Instant::now());
    let mut result = QueryResult::try_builder(&controls).expect("benchmark result admission");
    result
        .try_push_row(&[Value::Int64(
            i64::try_from(index).expect("benchmark cache index should fit i64"),
        )])
        .expect("benchmark cache row");
    result
        .try_set_command("SELECT 1")
        .expect("benchmark cache command");
    result.finish().expect("benchmark managed result")
}
