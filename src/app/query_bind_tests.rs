//! Rebuildable plan epochs cannot bypass the new concrete integer signatures.
use super::*;
use crate::runtime::query_cache;
use crate::sql::ast::{BinaryOp, Expr, SelectItem};
use crate::types::Value;
use std::path::PathBuf;
use std::sync::Arc;

struct Fixture {
    cassie: Option<Cassie>,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("cassie-bind-cache-{}", uuid::Uuid::new_v4()));
        let mut config = crate::config::CassieRuntimeConfig::default();
        config.limits.cf2_plan_ttl_seconds = 60;
        let cassie = Cassie::new_with_data_dir_and_config(path.to_str().expect("path"), config)
            .expect("Cassie");
        Self {
            cassie: Some(cassie),
            path,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        if self.path.exists() {
            std::fs::remove_dir_all(&self.path).expect("retired cache fixture");
        }
    }
}
struct Evidence {
    legacy_rows: Vec<Vec<Value>>,
    error: CassieError,
    misses: u64,
    live_bytes: usize,
    legacy_readable: bool,
}

fn without_integer_signature(expr: &Expr) -> Expr {
    let mut expr = expr.map_children(without_integer_signature);
    if let Expr::Binary { op, .. } = &mut expr {
        if matches!(op, BinaryOp::Integer { .. }) {
            *op = BinaryOp::Add;
        }
    }
    expr
}

fn probe(persisted: bool) -> Evidence {
    let fixture = Fixture::new();
    let cassie = fixture.cassie.as_ref().expect("live Cassie");
    let session = cassie.create_session("tester", None);
    let parsed = crate::sql::parse_statement("SELECT 2147483647::INT+1 AS n").expect("parse");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let controls = cassie.runtime.query_controls(Instant::now());
    let current = cassie
        .compile_physical_plan(parsed.clone(), Some(&session), Some(&controls))
        .expect("compile");
    let mut logical = current.logical.clone();
    for item in &mut logical.projection {
        if let SelectItem::Expr { expr, .. } = item {
            *expr = without_integer_signature(expr);
        }
    }
    let legacy = Arc::new(crate::planner::physical::build(logical));
    let legacy_rows = cassie
        .execute_physical_statement(&session, &legacy, Vec::new(), &controls)
        .expect("old int8 behavior")
        .rows()
        .to_vec();
    let mut key = cassie.plan_cache_key_from_fingerprint(
        fingerprint,
        Vec::new(),
        ExecutionMode::ExtendedQuery,
        session.database.clone(),
        &session.search_path(),
    );
    key.cost_model_version = 4;
    if persisted {
        let outcome = query_cache::observe_non_durable_plan_usage(
            &cassie.midge,
            &cassie.runtime,
            &key,
            &legacy,
            true,
        )
        .expect("persist legacy plan");
        assert!(matches!(
            outcome,
            query_cache::NonDurablePlanOutcome::Durable
        ));
    } else {
        cassie
            .runtime
            .plan_cache_store(key.clone(), Arc::clone(&legacy), true);
    }
    let before = cassie.runtime.snapshot().plan_cache.misses;
    let error = cassie
        .validate_pgwire_bind_with_controls(&session, parsed, fingerprint, &[], &controls)
        .expect_err("current int4 signature overflows");
    let legacy_readable = if persisted {
        query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &key)
            .expect("legacy derived record")
            .is_some()
    } else {
        cassie.runtime.plan_cache_lookup(&key).is_some()
    };
    Evidence {
        legacy_rows,
        error,
        misses: cassie.runtime.snapshot().plan_cache.misses - before,
        live_bytes: controls.current_query_memory_bytes(),
        legacy_readable,
    }
}

#[test]
fn should_reject_legacy_l1_integer_plans_given_bind_validation_when_current_signatures_overflow() {
    // Arrange
    let persisted = false;
    // Act
    let evidence = probe(persisted);
    // Assert
    assert_eq!(
        evidence.legacy_rows,
        vec![vec![Value::Int64(2_147_483_648)]]
    );
    assert_eq!(evidence.error.descriptor().sql_state, "22003");
    assert!(evidence.misses > 0);
    assert_eq!(evidence.live_bytes, 0);
    assert!(evidence.legacy_readable);
}

#[test]
fn should_reject_legacy_l2_integer_plans_given_bind_validation_when_current_signatures_overflow() {
    // Arrange
    let persisted = true;
    // Act
    let evidence = probe(persisted);
    // Assert
    assert_eq!(
        evidence.legacy_rows,
        vec![vec![Value::Int64(2_147_483_648)]]
    );
    assert_eq!(evidence.error.descriptor().sql_state, "22003");
    assert!(evidence.misses > 0);
    assert_eq!(evidence.live_bytes, 0);
    assert!(evidence.legacy_readable);
}
