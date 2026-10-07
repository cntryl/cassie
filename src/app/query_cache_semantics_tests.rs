//! Prior semantic-version plans must miss both compiled-plan cache layers.
use super::*;
use crate::planner::physical::PhysicalPlan;
use crate::sql::ast::{Expr, SelectItem};

const SQL: &str = "SELECT COALESCE(9007199254740993, 0.0) - 9007199254740992 AS value";

#[derive(Clone, Copy, Debug)]
enum Layer {
    L1,
    L2,
}
struct Evidence {
    old_rows: Vec<Vec<Value>>,
    rows: Vec<Vec<Value>>,
    compile_misses: u64,
    fresh_plan: bool,
    old_plan_still_readable: bool,
}
fn strip_float_anchor(expression: &Expr, removed: &mut usize) -> Expr {
    let mut expression = expression.map_children(|child| strip_float_anchor(child, removed));
    if let Expr::Function(function) = &mut expression {
        if function.name.eq_ignore_ascii_case("coalesce")
            && crate::sql::binder::coalesce_has_float_domain(function)
        {
            function.args.pop();
            *removed += 1;
        }
    }
    expression
}
fn legacy_plan(
    cassie: &Cassie,
    session: &CassieSession,
    parsed: &crate::sql::ast::ParsedStatement,
) -> Arc<PhysicalPlan> {
    let fresh = cassie
        .compile_physical_plan(parsed.clone(), Some(session), None)
        .expect("compile current semantics");
    let mut logical = fresh.logical.clone();
    let mut removed = 0;
    for item in &mut logical.projection {
        if let SelectItem::Expr { expr, .. } = item {
            *expr = strip_float_anchor(expr, &mut removed);
        }
    }
    assert_eq!(removed, 1, "fixture reproduces the pre-marker compiled AST");
    Arc::new(crate::planner::physical::build(logical))
}
fn probe(layer: Layer) -> Evidence {
    let path = std::env::temp_dir().join(format!("cassie-semantic-cache-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.cf2_plan_ttl_seconds = 60;
    let cassie =
        Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config).expect("Cassie");
    let session = cassie.create_session("tester", None);
    let parsed = crate::sql::parser::parse_statement(SQL).expect("parsed SQL");
    let current_key = cassie
        .query_cache_context(
            &session,
            &parsed,
            crate::runtime::sql_fingerprint(&parsed),
            &[],
            ExecutionMode::SimpleQuery,
            &[],
        )
        .cache_key
        .expect("cacheable");
    let mut old_key = current_key.clone();
    old_key.cost_model_version = 3;
    let old = legacy_plan(&cassie, &session, &parsed);
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let old_rows = cassie
        .execute_physical_statement(&session, &old, vec![], &controls)
        .expect("legacy engine path")
        .rows;
    match layer {
        Layer::L1 => cassie
            .runtime
            .plan_cache_store(old_key.clone(), Arc::clone(&old), true),
        Layer::L2 => {
            let outcome = query_cache::observe_non_durable_plan_usage(
                &cassie.midge,
                &cassie.runtime,
                &old_key,
                &old,
                true,
            )
            .expect("persist prior-version plan");
            assert!(matches!(
                outcome,
                query_cache::NonDurablePlanOutcome::Durable
            ));
            assert!(
                query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &old_key)
                    .expect("read prior artifact")
                    .is_some()
            );
            assert!(
                cassie.runtime.plan_cache_lookup(&old_key).is_none(),
                "persisted-only fixture"
            );
        }
    }
    let before = cassie.runtime.snapshot().plan_cache.misses;
    let rows = cassie
        .execute_sql(&session, SQL, vec![])
        .expect("actual cached query path")
        .rows;
    let (resolved, _) = cassie
        .resolve_physical_plan_with_parameter_oids(
            parsed,
            current_key,
            Some(&session),
            Some(&controls),
            &[],
        )
        .expect("read chosen plan");
    let old_plan_still_readable = match layer {
        Layer::L1 => cassie.runtime.plan_cache_lookup(&old_key).is_some(),
        Layer::L2 => query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &old_key)
            .expect("old derived artifact remains isolated")
            .is_some(),
    };
    let evidence = Evidence {
        old_rows,
        rows,
        compile_misses: cassie.runtime.snapshot().plan_cache.misses - before,
        fresh_plan: !Arc::ptr_eq(&resolved, &old)
            && resolved.logical.projection.iter().any(|item| {
                let SelectItem::Expr { expr, .. } = item else {
                    return false;
                };
                expr.any_descendant_or_self(&mut |expr| {
                    matches!(expr, Expr::Function(function) if crate::sql::binder::coalesce_has_float_domain(function))
                })
            }),
        old_plan_still_readable,
    };
    println!(
        "{layer:?} legacy rows={:?}, actual rows={:?}, compile misses={}",
        evidence.old_rows, evidence.rows, evidence.compile_misses
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop((controls, session, cassie));
    std::fs::remove_dir_all(path).expect("cleanup");
    evidence
}
#[test]
fn should_recompile_old_l1_coalesce_plans_before_executing_resolved_float_arithmetic() {
    // Arrange
    let layer = Layer::L1;
    // Act
    let evidence = probe(layer);
    // Assert
    assert_eq!(evidence.rows, vec![vec![Value::Float64(0.0)]]);
    assert_eq!(evidence.old_rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.fresh_plan);
    assert!(evidence.old_plan_still_readable);
}
#[test]
fn should_recompile_old_l2_coalesce_plans_before_executing_resolved_float_arithmetic() {
    // Arrange
    let layer = Layer::L2;
    // Act
    let evidence = probe(layer);
    // Assert
    assert_eq!(evidence.rows, vec![vec![Value::Float64(0.0)]]);
    assert_eq!(evidence.old_rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(evidence.compile_misses, 1);
    assert!(evidence.fresh_plan);
    assert!(evidence.old_plan_still_readable);
}
