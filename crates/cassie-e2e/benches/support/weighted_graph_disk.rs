//! Local storage counter evidence for the fixed Tier 3 weighted route.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

use super::{
    collect_weighted_evidence, fixture_identity, graph_evidence, graph_social, workloads, Fixture,
    SOCIAL_EDGES, SOCIAL_INDEXES, SOCIAL_MAPPED_ROWS, SOCIAL_PEOPLE,
};

#[path = "weighted_graph_disk_checks.rs"]
mod checks;

struct StoreDir(PathBuf);

impl StoreDir {
    fn create() -> Self {
        let path = std::env::temp_dir().join(format!("cassie-graph-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create isolated graph store");
        Self(path)
    }
}

impl Drop for StoreDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("cannot clean isolated graph store {:?}: {error}", self.0);
        }
    }
}

fn open(path: &Path) -> Fixture {
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_memory_budget_bytes = workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.query_timeout_ms = workloads::LARGE_ANALYTICAL_BENCHMARK_QUERY_TIMEOUT_MS;
    let cassie = Cassie::new_local_with_data_dir_and_config(path, config)
        .expect("open local weighted graph store");
    cassie.startup().expect("start local weighted graph store");
    let session = cassie.create_session("benchmark", None);
    Fixture {
        cassie: Arc::new(cassie),
        session,
    }
}

pub(super) fn run() {
    let store = StoreDir::create();
    let social = graph_social::Social::new(SOCIAL_PEOPLE, SOCIAL_EDGES);
    {
        let fixture = open(&store.0);
        social.plan().apply(&fixture.cassie, &fixture.session);
        fixture.cassie.shutdown();
    }
    let fixture = open(&store.0);
    let limits = fixture.cassie.runtime().limits();
    let native = fixture.cassie.midge.native_memory_configuration();
    assert_eq!(native.mode, "local", "reopened graph storage mode");
    assert_eq!(
        u64::try_from(limits.query_memory_budget_bytes).expect("query budget fits u64"),
        graph_evidence::BUDGET_BYTES,
        "reopened graph query budget"
    );
    assert!(!limits.execution_result_cache_enabled.is_enabled());
    let mut identity = fixture_identity(SOCIAL_MAPPED_ROWS, SOCIAL_INDEXES);
    identity["storage_mode"] = serde_json::json!(native.mode);
    identity["native_memory_configuration"] = serde_json::json!(native);
    identity["snapshot"] = serde_json::json!("unchanged committed fixture after close and reopen");
    identity["cold_scope"] = serde_json::json!("first query on reopened engine");
    identity["warm_scope"] = serde_json::json!("same query immediately afterward");
    identity["query_timeout_ms"] = serde_json::json!(limits.query_timeout_ms);

    let mut evidence = graph_evidence::Evidence::new("tier3_system_graph_disk_reopen");
    let (cold, warm) = collect_weighted_evidence(&mut evidence, &fixture, &social, identity);
    checks::should_validate_and_expand_the_weighted_graph_after_reopen(
        &mut evidence,
        u64::try_from(SOCIAL_EDGES).expect("positive fixture edge count"),
        cold,
        warm,
    );
    fixture.cassie.shutdown();
    evidence.finish();
}
