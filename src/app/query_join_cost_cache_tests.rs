//! Prior join-order plans must miss both compiled-plan cache layers.

use super::*;
use crate::catalog::canonical_relation_name;
use crate::planner::{optimizer, physical};
use crate::types::Value;

const SQL: &str = "SELECT cache_cost_a.id FROM cache_cost_a JOIN cache_cost_c ON cache_cost_a.id=cache_cost_c.ak JOIN cache_cost_b ON cache_cost_b.id=cache_cost_c.bk ORDER BY cache_cost_a.id";

#[derive(Clone, Copy)]
enum Layer {
    L1,
    L2,
}

struct Evidence {
    legacy_order: Vec<String>,
    fresh_order: Vec<String>,
    legacy_rows: Vec<Vec<Value>>,
    rows: Vec<Vec<Value>>,
    compile_misses: u64,
    old_plan_still_readable: bool,
}

fn memory_fixture() -> Cassie {
    let config = crate::config::CassieRuntimeConfig {
        password: uuid::Uuid::new_v4().to_string(),
        limits: crate::config::CassieRuntimeLimits {
            cf2_plan_ttl_seconds: 60,
            execution_result_cache_enabled: crate::config::ExecutionResultCacheEnabled::disabled(),
            ..crate::config::CassieRuntimeLimits::default()
        },
        ..crate::config::CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory cache fixture");
    cassie.startup().expect("start cache fixture");
    let session = cassie.create_session("root", None);
    for sql in [
        "CREATE TABLE cache_cost_a(id INT NOT NULL)",
        "CREATE TABLE cache_cost_b(id INT NOT NULL)",
        "CREATE TABLE cache_cost_c(ak INT NOT NULL,bk INT NOT NULL)",
        "INSERT INTO cache_cost_a VALUES(1),(2)",
        "INSERT INTO cache_cost_b VALUES(1),(2),(3)",
        "INSERT INTO cache_cost_c VALUES(1,1),(2,2),(3,3),(4,4)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("tiny cache fixture statement");
    }
    for name in ["cache_cost_a", "cache_cost_b", "cache_cost_c"] {
        let canonical = canonical_relation_name("postgres", "public", name);
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&canonical)
            .expect("fresh cache fixture statistics");
        cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
    }
    cassie
}

fn probe(layer: Layer) -> Evidence {
    let cassie = memory_fixture();
    let session = cassie.create_session("root", None);
    let parsed = crate::sql::parse_statement(SQL).expect("join SQL");
    let controls = cassie.runtime.query_controls(Instant::now());
    let current_key = cassie
        .query_cache_context_with_controls(
            &session,
            &parsed,
            crate::runtime::sql_fingerprint(&parsed),
            &[],
            (ExecutionMode::SimpleQuery, &[]),
            &controls,
        )
        .expect("admitted cache context")
        .cache_key
        .expect("cacheable join");
    let fresh = fresh_ndv_plan(&cassie, &session, &parsed, &controls);
    // An empty statistics map reproduces the prior lexical/cardinality choice.
    let legacy_logical = optimizer::optimize(fresh.logical.clone());
    let legacy_order = optimizer::join_order(&legacy_logical.source);
    let mut legacy = physical::build(legacy_logical);
    legacy
        .collection_schema
        .clone_from(&fresh.collection_schema);
    let legacy = Arc::new(legacy);
    let legacy_rows = cassie
        .execute_physical_statement(&session, &legacy, vec![], &controls)
        .expect("legacy join still returns the same rows")
        .rows()
        .to_vec();
    assert_eq!(
        legacy_order,
        ["cache_cost_a", "cache_cost_b", "cache_cost_c"],
        "prerequisite: the seeded plan has the actual prior join order"
    );
    assert_eq!(
        legacy_rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    let mut old_key = current_key.clone();
    old_key.cost_model_version = 36;
    seed_legacy_plan(&cassie, &old_key, &legacy, layer);
    let before = cassie.runtime.snapshot().plan_cache.misses;
    let rows = cassie
        .execute_sql(&session, SQL, vec![])
        .expect("normal compiled-plan cache path")
        .rows()
        .to_vec();
    let (resolved, _) = cassie
        .resolve_physical_plan_with_parameter_oids(
            parsed,
            current_key,
            Some(&session),
            Some(&controls),
            &[],
        )
        .expect("read the selected compiled plan");
    let old_plan_still_readable = match layer {
        Layer::L1 => cassie.runtime.plan_cache_lookup(&old_key).is_some(),
        Layer::L2 => query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &old_key)
            .expect("old derived plan remains isolated")
            .is_some(),
    };
    let evidence = Evidence {
        legacy_order,
        fresh_order: optimizer::join_order(&resolved.logical.source),
        legacy_rows,
        rows,
        compile_misses: cassie.runtime.snapshot().plan_cache.misses - before,
        old_plan_still_readable,
    };
    drop((resolved, fresh, legacy, session, cassie));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    evidence
}

fn fresh_ndv_plan(
    cassie: &Cassie,
    session: &CassieSession,
    parsed: &crate::sql::ast::ParsedStatement,
    controls: &QueryExecutionControls,
) -> Arc<physical::PhysicalPlan> {
    let fresh = cassie
        .compile_physical_plan(parsed.clone(), Some(session), Some(controls))
        .expect("compile source logical tree");
    assert_eq!(
        optimizer::join_order(&fresh.logical.source),
        ["cache_cost_a", "cache_cost_c", "cache_cost_b"],
        "prerequisite: fresh compilation selects the NDV order before the cache RED gate"
    );
    assert_eq!(
        cassie
            .execute_physical_statement(session, &fresh, vec![], controls)
            .expect("fresh NDV plan executes before seeding")
            .rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    fresh
}

fn seed_legacy_plan(
    cassie: &Cassie,
    old_key: &PlanCacheKey,
    legacy: &Arc<physical::PhysicalPlan>,
    layer: Layer,
) {
    match layer {
        Layer::L1 => {
            cassie
                .runtime
                .plan_cache_store(old_key.clone(), Arc::clone(legacy), true);
            let stored = cassie
                .runtime
                .plan_cache_lookup(old_key)
                .expect("read seeded L1 plan");
            assert_eq!(
                optimizer::join_order(&stored.plan.logical.source),
                ["cache_cost_a", "cache_cost_b", "cache_cost_c"]
            );
        }
        Layer::L2 => {
            let outcome = query_cache::observe_non_durable_plan_usage(
                &cassie.midge,
                &cassie.runtime,
                old_key,
                legacy,
                true,
            )
            .expect("store prior compiled join in L2");
            assert!(matches!(
                outcome,
                query_cache::NonDurablePlanOutcome::Durable
            ));
            assert!(cassie.runtime.plan_cache_lookup(old_key).is_none());
            let stored = query_cache::lookup_plan(&cassie.midge, &cassie.runtime, old_key)
                .expect("read prior artifact");
            assert!(stored.is_some());
            assert_eq!(
                optimizer::join_order(&stored.expect("seeded L2 plan").logical.source),
                ["cache_cost_a", "cache_cost_b", "cache_cost_c"]
            );
        }
    }
}

#[test]
fn should_recompile_a_prior_l1_join_order_before_using_fresh_selectivity() {
    // Arrange
    let layer = Layer::L1;

    // Act
    let evidence = probe(layer);

    // Assert
    assert_eq!(
        evidence.legacy_order,
        ["cache_cost_a", "cache_cost_b", "cache_cost_c"]
    );
    assert_eq!(
        evidence.fresh_order,
        ["cache_cost_a", "cache_cost_c", "cache_cost_b"]
    );
    assert_eq!(
        evidence.rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(evidence.legacy_rows, evidence.rows);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.old_plan_still_readable);
}

#[test]
fn should_recompile_a_prior_l2_join_order_before_using_fresh_selectivity() {
    // Arrange
    let layer = Layer::L2;

    // Act
    let evidence = probe(layer);

    // Assert
    assert_eq!(
        evidence.legacy_order,
        ["cache_cost_a", "cache_cost_b", "cache_cost_c"]
    );
    assert_eq!(
        evidence.fresh_order,
        ["cache_cost_a", "cache_cost_c", "cache_cost_b"]
    );
    assert_eq!(
        evidence.rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(evidence.legacy_rows, evidence.rows);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.old_plan_still_readable);
}
