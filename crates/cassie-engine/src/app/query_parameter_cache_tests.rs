//! Rebuildable persisted plans must retain exact effective parameter identities.

use super::*;
use crate::runtime::query_cache;
use crate::types::Value;

fn persisted_probe(legacy: bool) -> (Vec<Vec<Value>>, u64, bool) {
    let path =
        std::env::temp_dir().join(format!("cassie-parameter-cache-{}", uuid::Uuid::new_v4()));
    let config = crate::config::CassieRuntimeConfig {
        limits: crate::config::CassieRuntimeLimits {
            cf2_plan_ttl_seconds: 60,
            ..crate::config::CassieRuntimeLimits::default()
        },
        ..crate::config::CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("persisted cache");
    let session = cassie.create_session("tester", None);
    let sql = "SELECT $1::BYTEA AS value";
    let parsed = crate::sql::parse_statement(sql).expect("parameter SQL");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let controls = cassie.runtime.query_controls(Instant::now());
    let parameter = [Value::Int64(-1)];
    let plan = cassie
        .compile_physical_plan_with_parameter_oids(
            parsed.clone(),
            Some(&session),
            Some(&controls),
            &[21],
        )
        .expect("compile narrow source plan");
    let mut source_key = cassie
        .query_cache_context_with_controls(
            &session,
            &parsed,
            fingerprint,
            &parameter,
            (ExecutionMode::ExtendedQuery, &[21]),
            &controls,
        )
        .expect("source cache identity")
        .cache_key
        .expect("cacheable source");
    if legacy {
        source_key.sql_fingerprint = fingerprint;
        source_key.cost_model_version = 31;
    }
    let outcome = query_cache::observe_non_durable_plan_usage(
        &cassie.midge,
        &cassie.runtime,
        &source_key,
        &plan,
        true,
    )
    .expect("persist source plan");
    assert!(matches!(
        outcome,
        query_cache::NonDurablePlanOutcome::Durable
    ));
    assert!(cassie.runtime.plan_cache_lookup(&source_key).is_none());
    let before = cassie.runtime.snapshot().plan_cache.misses;
    let rows = cassie
        .execute_parsed_statement_core_with_parameter_oids(
            &session,
            parsed,
            fingerprint,
            (parameter.to_vec(), &[20]),
            ExecutionMode::ExtendedQuery,
            &controls,
        )
        .expect("execute wide target parameter")
        .rows()
        .to_vec();
    let misses = cassie.runtime.snapshot().plan_cache.misses - before;
    let source_readable = query_cache::lookup_plan(&cassie.midge, &cassie.runtime, &source_key)
        .expect("source artifact remains readable")
        .is_some();
    drop((plan, session, cassie));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(controls);
    std::fs::remove_dir_all(path).expect("persisted fixture cleanup");
    (rows, misses, source_readable)
}

#[test]
fn should_isolate_persisted_plans_by_exact_parameter_oids() {
    // Arrange
    let legacy = false;
    // Act
    let (rows, misses, source_readable) = persisted_probe(legacy);
    // Assert
    assert_eq!(
        rows,
        [vec![Value::String("\\xffffffffffffffff".to_owned())]]
    );
    assert_eq!(misses, 1);
    assert!(source_readable);
}

#[test]
fn should_recompile_persisted_carrier_only_parameter_plans() {
    // Arrange
    let legacy = true;
    // Act
    let (rows, misses, source_readable) = persisted_probe(legacy);
    // Assert
    assert_eq!(
        rows,
        [vec![Value::String("\\xffffffffffffffff".to_owned())]]
    );
    assert_eq!(misses, 1);
    assert!(source_readable);
}

#[test]
fn should_isolate_execution_results_by_exact_parameter_oids() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("result cache engine");
    let session = cassie.create_session("tester", None);
    let parsed = crate::sql::parse_statement("SELECT $1::BYTEA AS value").expect("result SQL");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let controls = cassie.runtime.query_controls(Instant::now());
    // Act
    let rows = [21, 23, 20, 20].map(|oid| {
        cassie
            .execute_parsed_statement_core_with_parameter_oids(
                &session,
                parsed.clone(),
                fingerprint,
                (vec![Value::Int64(-1)], &[oid]),
                ExecutionMode::ExtendedQuery,
                &controls,
            )
            .expect("typed result")
            .rows()
            .to_vec()
    });
    let metrics = cassie.runtime.snapshot().execution_result_cache;
    // Assert
    assert_eq!(
        rows,
        [
            vec![vec![Value::String("\\xffff".to_owned())]],
            vec![vec![Value::String("\\xffffffff".to_owned())]],
            vec![vec![Value::String("\\xffffffffffffffff".to_owned())]],
            vec![vec![Value::String("\\xffffffffffffffff".to_owned())]],
        ],
    );
    assert_eq!(metrics.misses, 3);
    assert_eq!(metrics.hits, 1);
}

#[test]
fn should_isolate_result_cache_keys_by_ordered_parameter_oids() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("ordered result key engine");
    let session = cassie.create_session("tester", None);
    let parsed = crate::sql::parse_statement("SELECT $1 AS a, $2 AS b").expect("ordered SQL");
    let fingerprint = crate::runtime::sql_fingerprint(&parsed);
    let controls = cassie.runtime.query_controls(Instant::now());
    let parameters = [Value::Int64(1), Value::Int64(1)];
    // Act
    let contexts = [[21, 20], [20, 21]].map(|oids| {
        cassie
            .query_cache_context_with_controls(
                &session,
                &parsed,
                fingerprint,
                &parameters,
                (ExecutionMode::ExtendedQuery, &oids),
                &controls,
            )
            .expect("ordered cache identity")
    });
    // Assert
    assert_ne!(contexts[0].exec_cache_key, contexts[1].exec_cache_key);
}

#[test]
fn should_isolate_bytea_execution_results_from_text() {
    // Arrange
    let orders = [[17, 25, 25], [25, 17, 17]];
    // Act
    let evidence = orders.map(|order| {
        let cassie =
            Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
                .expect("bytea result cache engine");
        let session = cassie.create_session("tester", None);
        let parsed = crate::sql::parse_statement("SELECT $1 || $2 AS value").expect("concat SQL");
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        let controls = cassie.runtime.query_controls(Instant::now());
        let rows = order.map(|oid| {
            cassie
                .execute_parsed_statement_core_with_parameter_oids(
                    &session,
                    parsed.clone(),
                    fingerprint,
                    (
                        vec![
                            Value::String("\\x01".to_owned()),
                            Value::String("\\x02".to_owned()),
                        ],
                        &[oid, oid],
                    ),
                    ExecutionMode::ExtendedQuery,
                    &controls,
                )
                .expect("typed concat result")
                .rows()
                .to_vec()
        });
        (rows, cassie.runtime.snapshot().execution_result_cache)
    });
    // Assert
    for (order, (rows, metrics)) in orders.into_iter().zip(evidence) {
        for (oid, rows) in order.into_iter().zip(rows) {
            let expected = if oid == 17 { "\\x0102" } else { "\\x01\\x02" };
            assert_eq!(rows, [vec![Value::String(expected.to_owned())]]);
        }
        assert_eq!(metrics.misses, 2);
        assert_eq!(metrics.hits, 1);
    }
}

#[test]
fn should_preserve_empty_parameter_cache_fingerprints() {
    // Arrange
    let fingerprint = 42;
    // Act
    let actual = super::super::query_parameters::parameter_cache_fingerprint(fingerprint, &[]);
    // Assert
    assert_eq!(actual, fingerprint);
}

#[test]
fn should_distinguish_ordered_parameter_cache_fingerprints() {
    // Arrange
    let fingerprint = 42;
    // Act
    let identities = [[21, 20], [20, 21]].map(|oids| {
        super::super::query_parameters::parameter_cache_fingerprint(fingerprint, &oids)
    });
    // Assert
    assert_ne!(identities[0], identities[1]);
}

#[test]
fn should_report_parameterless_user_schema_diagnostic_cache_hits() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(crate::config::CassieRuntimeConfig::default())
        .expect("user schema diagnostic engine");
    cassie.startup().expect("diagnostic startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE SCHEMA tester",
        "CREATE FUNCTION tester.lower(x BYTEA) RETURNS BYTEA AS \"x\"",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("scoped UDF");
    }
    session.set_search_path(vec!["$user".to_owned(), "public".to_owned()]);
    let sql = "SELECT lower('\\x01'::BYTEA) AS value";
    let rows = cassie
        .execute_sql(&session, sql, vec![])
        .expect("user schema overload")
        .rows()
        .to_vec();
    let parsed = crate::sql::parse_statement(sql).expect("scoped diagnostic SQL");
    let path = session.search_path();
    let controls = cassie.runtime.query_controls(Instant::now());
    let userless_scope = crate::catalog::FunctionInputScope {
        database: session.current_database().expect("database"),
        search_path: &path,
        user: "",
        enforce_database_scope: true,
    };
    // Act
    let userless_admission = crate::sql::checked_parameter_types_in_scope(
        &parsed,
        crate::sql::PrivateParameterTypeInput::Embedded {
            effective: &[],
            values: &[],
        },
        &cassie.catalog,
        &controls,
        Some(&userless_scope),
    );
    let hit = cassie.plan_cache_hit_for_diagnostics(
        &parsed,
        &[],
        ExecutionMode::SimpleQuery,
        session.database.clone(),
        &path,
    );
    // Assert
    assert_eq!(rows, [vec![Value::String("\\x01".to_owned())]]);
    assert!(userless_admission.is_err(), "the builtin rejects BYTEA");
    assert!(
        hit,
        "parameterless lookup must preserve its existing identity"
    );
}
