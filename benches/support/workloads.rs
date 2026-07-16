#![allow(dead_code, unused_imports)]

#[path = "workloads/bound_sql.rs"]
mod bound_sql;
#[path = "workloads/context.rs"]
mod context;
#[path = "workloads/empty_context.rs"]
mod empty_context;
#[path = "workloads/hotpath.rs"]
mod hotpath;
#[path = "workloads/http.rs"]
mod http;
#[path = "workloads/join_context.rs"]
mod join_context;
#[path = "workloads/lifecycle.rs"]
mod lifecycle;
#[path = "workloads/mock_tei.rs"]
mod mock_tei;
#[path = "workloads/mock_tei_context.rs"]
mod mock_tei_context;
#[path = "workloads/pgwire.rs"]
mod pgwire;
#[path = "workloads/scaling.rs"]
mod scaling;
#[path = "workloads/scaling_legacy.rs"]
mod scaling_legacy;
#[path = "workloads/sql.rs"]
mod sql;
#[path = "workloads/subsystem.rs"]
mod subsystem;
#[path = "workloads/system.rs"]
mod system;
#[path = "workloads/tier3.rs"]
mod tier3;
#[path = "workloads/tier3_query_fixture.rs"]
mod tier3_query_fixture;

pub use bound_sql::{
    plan_cache_miss as bound_plan_cache_miss, recursive_cte as bound_recursive_cte,
    time_series_window as bound_time_series_window, BoundBenchmarkSql,
};
pub use context::{
    column_batch_context, context, disk_context_with_temp_budget, execution_result_cache_context,
    graph_context, recursive_cte_context, replay_context, runtime, scalar_context,
    time_series_context, time_series_disk_context_with_temp_budget, unindexed_context,
    unindexed_disk_context_with_temp_budget, worker_scaling_context, BenchContext,
    ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};
pub use empty_context::{empty_context, empty_context_with_temp_budget};
pub use hotpath::*;
pub use http::*;
pub use join_context::*;
pub use lifecycle::*;
pub use mock_tei_context::context_with_mock_tei_embeddings;
pub use pgwire::*;
pub use scaling::*;
pub use scaling_legacy::*;
pub use sql::*;
pub use subsystem::*;
pub use system::*;
pub use tier3::*;
pub use tier3_query_fixture::*;
