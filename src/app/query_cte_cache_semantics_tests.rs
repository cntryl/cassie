//! A schema-valid prior CTE binding must miss both compiled-plan caches.
//!
//! These probes require the corrected fresh binding before seeding epoch 35.
//! They are authored at T1 and intentionally first execute at the T2 cache gate.

use super::*;
use crate::types::Value;

const SQL: &str = "WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM \"Base\"";
const LEGACY_SQL: &str = "WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM public.\"Base\"";

#[derive(Clone, Copy)]
enum Layer {
    L1,
    L2,
}

struct Evidence {
    legacy_rows: Vec<Vec<Value>>,
    rows: Vec<Vec<Value>>,
    selected_rows: Vec<Vec<Value>>,
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
        "CREATE TABLE public.\"Base\"(x INT)",
        "INSERT INTO public.\"Base\" VALUES(2)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("tiny catalog fixture");
    }
    cassie
}

fn probe(layer: Layer) -> Evidence {
    let cassie = memory_fixture();
    let session = cassie.create_session("root", None);
    let parsed = crate::sql::parse_statement(SQL).expect("quoted CTE query");
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
        .expect("cacheable CTE");
    let fresh = cassie
        .compile_physical_plan(parsed.clone(), Some(&session), Some(&controls))
        .expect("compile target without cache lookup");
    assert_eq!(
        cassie
            .execute_physical_statement(&session, &fresh, vec![], &controls)
            .expect("fresh binding executes")
            .rows(),
        &[vec![Value::Int64(77)]],
        "prerequisite: runtime binding is corrected before the stale-cache RED gate"
    );

    // The old quoted-reference bug bound a real catalog collection. Compiling
    // the explicitly qualified equivalent preserves its actual schema metadata,
    // CTE definitions and executable collection source, rather than a sentinel.
    let legacy = cassie
        .compile_physical_plan(
            crate::sql::parse_statement(LEGACY_SQL).expect("legacy catalog binding"),
            Some(&session),
            Some(&controls),
        )
        .expect("schema-valid legacy plan");
    let legacy_rows = cassie
        .execute_physical_statement(&session, &legacy, vec![], &controls)
        .expect("legacy catalog plan executes")
        .rows()
        .to_vec();
    assert_eq!(legacy_rows, vec![vec![Value::Int64(2)]]);
    let mut old_key = current_key.clone();
    old_key.cost_model_version = 35;
    match layer {
        Layer::L1 => cassie
            .runtime
            .plan_cache_store(old_key.clone(), Arc::clone(&legacy), true),
        Layer::L2 => {
            let outcome = query_cache::observe_non_durable_plan_usage(
                &cassie.midge,
                &cassie.runtime,
                &old_key,
                &legacy,
                true,
            )
            .expect("store legacy bound CTE in L2");
            assert!(matches!(
                outcome,
                query_cache::NonDurablePlanOutcome::Durable
            ));
            assert!(cassie.runtime.plan_cache_lookup(&old_key).is_none());
            assert!(
                query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &old_key)
                    .expect("read old derived artifact")
                    .is_some()
            );
        }
    }
    let before = cassie.runtime.snapshot().plan_cache.misses;
    let rows = cassie
        .execute_sql(&session, SQL, vec![])
        .expect("actual target query cache path")
        .rows()
        .to_vec();
    let compile_misses = cassie.runtime.snapshot().plan_cache.misses - before;
    let (selected, _) = cassie
        .resolve_physical_plan_with_parameter_oids(
            parsed,
            current_key,
            Some(&session),
            Some(&controls),
            &[],
        )
        .expect("selected bound plan");
    let selected_rows = cassie
        .execute_physical_statement(&session, &selected, vec![], &controls)
        .expect("selected plan executes")
        .rows()
        .to_vec();
    let old_plan_still_readable = match layer {
        Layer::L1 => cassie.runtime.plan_cache_lookup(&old_key).is_some(),
        Layer::L2 => query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &old_key)
            .expect("old artifact remains isolated")
            .is_some(),
    };
    drop((selected, fresh, legacy, session, cassie));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    Evidence {
        legacy_rows,
        rows,
        selected_rows,
        compile_misses,
        old_plan_still_readable,
    }
}

#[test]
fn should_recompile_a_prior_l1_cte_binding() {
    // Arrange
    let layer = Layer::L1;

    // Act
    let evidence = probe(layer);

    // Assert
    assert_eq!(evidence.legacy_rows, vec![vec![Value::Int64(2)]]);
    assert_eq!(evidence.rows, vec![vec![Value::Int64(77)]]);
    assert_eq!(evidence.selected_rows, evidence.rows);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.old_plan_still_readable);
}

#[test]
fn should_recompile_a_prior_l2_cte_binding() {
    // Arrange
    let layer = Layer::L2;

    // Act
    let evidence = probe(layer);

    // Assert
    assert_eq!(evidence.legacy_rows, vec![vec![Value::Int64(2)]]);
    assert_eq!(evidence.rows, vec![vec![Value::Int64(77)]]);
    assert_eq!(evidence.selected_rows, evidence.rows);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.old_plan_still_readable);
}
