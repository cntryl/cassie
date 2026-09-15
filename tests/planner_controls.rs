// Consolidated integration suite: planner_controls.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/metrics.rs"]
mod support_metrics;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/query_evidence.rs"]
mod support_query_evidence;
#[path = "support/sql.rs"]
mod support_sql;

// Formerly tests/adaptive_promotion_evidence.rs.
mod adaptive_promotion_evidence {
    use cassie::config::{CassieRuntimeLimits, OperatorSwitchingEnabled};

    #[test]
    fn should_keep_adaptive_controls_disabled_by_default() {
        // Arrange
        let limits = CassieRuntimeLimits::default();

        // Act
        let adaptive_enabled = limits.adaptive_execution_enabled;
        let operator_switching_enabled = limits.operator_switching_enabled;

        // Assert
        assert!(!adaptive_enabled);
        assert_eq!(
            operator_switching_enabled,
            OperatorSwitchingEnabled::disabled()
        );
    }
}

// Formerly tests/execution_result_cache.rs.
mod execution_result_cache {
    use super::support_sql as support;
    use cassie::types::Value;
    use cassie::{Cassie, CassieRuntimeConfig};
    use std::sync::{Arc, Barrier};
    use support::*;

    fn cache_config(max_entries: usize, max_bytes: usize) -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_max_entries = max_entries;
        config.limits.execution_result_cache_max_bytes = max_bytes;
        config
    }

    #[test]
    fn should_isolate_current_user_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("current-user");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let alice = cassie.create_session("alice", None);
        let bob = cassie.create_session("bob", None);
        cassie
            .execute_sql(&alice, "CREATE TABLE cache_users (marker TEXT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(
                &alice,
                "INSERT INTO cache_users (marker) VALUES ('one')",
                vec![],
            )
            .expect("seed row");

        // Act
        let alice_result = cassie
            .execute_sql(
                &alice,
                "SELECT marker, current_user() FROM cache_users",
                vec![],
            )
            .expect("alice query");
        let bob_result = cassie
            .execute_sql(
                &bob,
                "SELECT marker, current_user() FROM cache_users",
                vec![],
            )
            .expect("bob query");

        // Assert
        assert_eq!(
            alice_result.rows,
            vec![vec![
                Value::String("one".into()),
                Value::String("alice".into())
            ]]
        );
        assert_eq!(
            bob_result.rows,
            vec![vec![
                Value::String("one".into()),
                Value::String("bob".into())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_isolate_execution_results_given_different_authenticated_roles() {
        // Arrange
        use_local_storage();
        let path = data_dir("authenticated-roles");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let alice = cassie.create_session("alice", None);
        let bob = cassie.create_session("bob", None);
        cassie
            .execute_sql(&alice, "CREATE TABLE cache_role_rows (value TEXT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(
                &alice,
                "INSERT INTO cache_role_rows (value) VALUES ('shared')",
                vec![],
            )
            .expect("seed row");

        // Act
        let alice_result = cassie
            .execute_sql(&alice, "SELECT value FROM cache_role_rows", vec![])
            .expect("alice query");
        let bob_result = cassie
            .execute_sql(&bob, "SELECT value FROM cache_role_rows", vec![])
            .expect("bob query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(alice_result.rows, bob_result.rows);
        assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(
            metrics["execution_result_cache"]["misses"].as_u64(),
            Some(2)
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_isolate_execution_results_given_different_search_paths() {
        // Arrange
        use_local_storage();
        let path = data_dir("search-paths");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        for sql in [
            "CREATE SCHEMA alpha",
            "CREATE SCHEMA beta",
            "CREATE TABLE alpha.cache_path_rows (value TEXT)",
            "CREATE TABLE beta.cache_path_rows (value TEXT)",
            "INSERT INTO alpha.cache_path_rows (value) VALUES ('alpha')",
            "INSERT INTO beta.cache_path_rows (value) VALUES ('beta')",
        ] {
            cassie
                .execute_sql(&setup, sql, vec![])
                .expect("setup schema");
        }
        let alpha = cassie.create_session("reader", None);
        let beta = cassie.create_session("reader", None);
        cassie
            .execute_sql(&alpha, "SET search_path TO alpha", vec![])
            .expect("alpha path");
        cassie
            .execute_sql(&beta, "SET search_path TO beta", vec![])
            .expect("beta path");

        // Act
        let alpha_result = cassie
            .execute_sql(&alpha, "SELECT value FROM cache_path_rows", vec![])
            .expect("alpha query");
        let beta_result = cassie
            .execute_sql(&beta, "SELECT value FROM cache_path_rows", vec![])
            .expect("beta query");

        // Assert
        assert_eq!(alpha_result.rows, vec![vec![Value::String("alpha".into())]]);
        assert_eq!(beta_result.rows, vec![vec![Value::String("beta".into())]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_isolate_execution_results_given_different_session_settings() {
        // Arrange
        use_local_storage();
        let path = data_dir("session-settings");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let first = cassie.create_session("reader", None);
        let second = cassie.create_session("reader", None);
        cassie
            .execute_sql(&first, "SET application_name TO first_client", vec![])
            .expect("first setting");
        cassie
            .execute_sql(&second, "SET application_name TO second_client", vec![])
            .expect("second setting");

        // Act
        let first_result = cassie
            .execute_sql(&first, "SELECT current_setting('application_name')", vec![])
            .expect("first setting query");
        let second_result = cassie
            .execute_sql(
                &second,
                "SELECT current_setting('application_name')",
                vec![],
            )
            .expect("second setting query");

        // Assert
        assert_eq!(
            first_result.rows,
            vec![vec![Value::String("first_client".into())]]
        );
        assert_eq!(
            second_result.rows,
            vec![vec![Value::String("second_client".into())]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bypass_cached_results_during_transaction() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(&session, "CREATE TABLE cache_tx_docs (title TEXT)", vec![])
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cache_tx_docs (title) VALUES ('before')",
                vec![],
            )
            .expect("seed row");
        let _ = cassie
            .execute_sql(
                &session,
                "SELECT title FROM cache_tx_docs ORDER BY title",
                vec![],
            )
            .expect("warm result cache");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cache_tx_docs (title) VALUES ('during')",
                vec![],
            )
            .expect("stage row");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM cache_tx_docs ORDER BY title",
                vec![],
            )
            .expect("transaction query");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("before".into())],
                vec![Value::String("during".into())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bypass_non_immutable_user_functions() {
        // Arrange
        use_local_storage();
        let path = data_dir("stable-udf");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(
                &session,
                r#"CREATE FUNCTION stable_echo(x TEXT) RETURNS TEXT STABLE AS "x""#,
                vec![],
            )
            .expect("create function");

        // Act
        let first = cassie
            .execute_sql(&session, "SELECT stable_echo('value')", vec![])
            .expect("first query");
        let second = cassie
            .execute_sql(&session, "SELECT stable_echo('value')", vec![])
            .expect("second query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows, second.rows);
        assert_eq!(
            metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
            Some(2)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bypass_volatile_user_functions() {
        // Arrange
        use_local_storage();
        let path = data_dir("volatile-udf");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(
                &session,
                r#"CREATE FUNCTION volatile_echo(x TEXT) RETURNS TEXT VOLATILE AS "x""#,
                vec![],
            )
            .expect("create function");

        // Act
        let first = cassie
            .execute_sql(&session, "SELECT volatile_echo('value')", vec![])
            .expect("first query");
        let second = cassie
            .execute_sql(&session, "SELECT volatile_echo('value')", vec![])
            .expect("second query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows, second.rows);
        assert_eq!(
            metrics["execution_result_cache"]["bypass_reasons"]["non_immutable_function"].as_u64(),
            Some(2)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cache_immutable_user_functions() {
        // Arrange
        use_local_storage();
        let path = data_dir("immutable-udf");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(
                &session,
                r#"CREATE FUNCTION immutable_echo(x TEXT) RETURNS TEXT IMMUTABLE AS "x""#,
                vec![],
            )
            .expect("create function");

        // Act
        let first = cassie
            .execute_sql(&session, "SELECT immutable_echo('value')", vec![])
            .expect("first query");
        let second = cassie
            .execute_sql(&session, "SELECT immutable_echo('value')", vec![])
            .expect("second query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows, second.rows);
        assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_observe_savepoint_rollback_without_cache() {
        // Arrange
        use_local_storage();
        let path = data_dir("savepoint");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cache_savepoint (marker TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cache_savepoint (marker) VALUES ('before')",
                vec![],
            )
            .expect("seed row");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(&session, "SAVEPOINT cache_point", vec![])
            .expect("savepoint");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO cache_savepoint (marker) VALUES ('after')",
                vec![],
            )
            .expect("stage row");
        let before_rollback = cassie
            .execute_sql(
                &session,
                "SELECT marker FROM cache_savepoint ORDER BY marker",
                vec![],
            )
            .expect("read staged row");

        // Act
        cassie
            .execute_sql(&session, "ROLLBACK TO SAVEPOINT cache_point", vec![])
            .expect("rollback to savepoint");
        let after_rollback = cassie
            .execute_sql(
                &session,
                "SELECT marker FROM cache_savepoint ORDER BY marker",
                vec![],
            )
            .expect("read rolled back state");

        // Assert
        assert_eq!(before_rollback.rows.len(), 2);
        assert_eq!(
            after_rollback.rows,
            vec![vec![Value::String("before".into())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bypass_virtual_catalog_results() {
        // Arrange
        use_local_storage();
        let path = data_dir("virtual-catalog");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("alice", None);

        // Act
        let _ = cassie
            .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
            .expect("first catalog query");
        let _ = cassie
            .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
            .expect("second catalog query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            metrics["execution_result_cache"]["bypass_reasons"]["virtual_catalog"].as_u64(),
            Some(2)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_oversized_cache_entries() {
        // Arrange
        use_local_storage();
        let path = data_dir("oversized");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, cache_config(64, 1)).expect("cassie");
        let session = cassie.create_session("alice", None);

        // Act
        let _ = cassie
            .execute_sql(&session, "SELECT 'payload'", vec![])
            .expect("query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            metrics["execution_result_cache"]["entries"].as_u64(),
            Some(0)
        );
        assert_eq!(metrics["execution_result_cache"]["bytes"].as_u64(), Some(0));
        assert_eq!(
            metrics["execution_result_cache"]["bypass_reasons"]["oversized_entry"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_evict_least_recently_used_result() {
        // Arrange
        use_local_storage();
        let path = data_dir("lru");
        let cassie = Cassie::new_with_data_dir_and_config(&path, cache_config(2, 1_000_000))
            .expect("cassie");
        let session = cassie.create_session("alice", None);
        cassie
            .execute_sql(&session, "CREATE TABLE cache_lru (marker TEXT)", vec![])
            .expect("create table");
        for marker in ["a", "b", "c"] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO cache_lru (marker) VALUES ($1)",
                    vec![Value::String(marker.into())],
                )
                .expect("insert row");
        }
        let query = "SELECT marker FROM cache_lru WHERE marker = $1";
        for marker in ["a", "b", "a", "c", "b"] {
            cassie
                .execute_sql(&session, query, vec![Value::String(marker.into())])
                .expect("cached query");
        }

        // Act
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(1));
        assert_eq!(
            metrics["execution_result_cache"]["misses"].as_u64(),
            Some(4)
        );
        assert_eq!(
            metrics["execution_result_cache"]["evictions"].as_u64(),
            Some(2)
        );
        assert_eq!(
            metrics["execution_result_cache"]["entries"].as_u64(),
            Some(2)
        );
        assert!(metrics["execution_result_cache"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes > 0));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_evict_results_over_byte_budget() {
        // Arrange
        use_local_storage();
        let path = data_dir("byte-budget");
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, cache_config(64, 220)).expect("cassie");
        let session = cassie.create_session("alice", None);

        // Act
        cassie
            .execute_sql(&session, "SELECT 'first-payload'", vec![])
            .expect("first query");
        cassie
            .execute_sql(&session, "SELECT 'second-payload'", vec![])
            .expect("second query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            metrics["execution_result_cache"]["entries"].as_u64(),
            Some(1)
        );
        assert_eq!(
            metrics["execution_result_cache"]["evictions"].as_u64(),
            Some(1)
        );
        assert!(metrics["execution_result_cache"]["bytes"]
            .as_u64()
            .is_some_and(|bytes| bytes <= 220));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_remain_fresh_during_concurrent_invalidation() {
        // Arrange
        use_local_storage();
        let path = data_dir("concurrent-invalidation");
        let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
        let setup = cassie.create_session("setup", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE cache_concurrent (marker TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &setup,
                "INSERT INTO cache_concurrent (marker) VALUES ('before')",
                vec![],
            )
            .expect("seed row");
        let query = "SELECT marker FROM cache_concurrent ORDER BY marker";
        cassie
            .execute_sql(&setup, query, vec![])
            .expect("warm cache");
        let barrier = Arc::new(Barrier::new(2));

        // Act
        std::thread::scope(|scope| {
            let reader_cassie = Arc::clone(&cassie);
            let reader_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                let reader = reader_cassie.create_session("reader", None);
                reader_barrier.wait();
                for _ in 0..20 {
                    reader_cassie
                        .execute_sql(&reader, query, vec![])
                        .expect("concurrent read");
                }
            });
            let writer_cassie = Arc::clone(&cassie);
            let writer_barrier = Arc::clone(&barrier);
            scope.spawn(move || {
                let writer = writer_cassie.create_session("writer", None);
                writer_barrier.wait();
                writer_cassie
                    .execute_sql(
                        &writer,
                        "INSERT INTO cache_concurrent (marker) VALUES ('after')",
                        vec![],
                    )
                    .expect("concurrent write");
            });
        });
        let final_result = cassie
            .execute_sql(&setup, query, vec![])
            .expect("fresh final read");

        // Assert
        assert_eq!(
            final_result.rows,
            vec![
                vec![Value::String("after".into())],
                vec![Value::String("before".into())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/optimizer_memo.rs.
mod optimizer_memo {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::types::Value;
    use std::fmt::Write as _;

    use super::support_sql as support;
    use support::*;

    fn seed_relation(cassie: &Cassie, name: &str, rows: usize) {
        let session = cassie.create_session("seed", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {name} (item_key INT, parent_key INT)"),
                vec![],
            )
            .expect("create memo relation");
        for id in 1..=rows {
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO {name} (item_key, parent_key) VALUES ($1, $2)"),
                    vec![
                        cassie::types::Value::Int64(id.try_into().expect("fixture id")),
                        cassie::types::Value::Int64(id.try_into().expect("fixture parent id")),
                    ],
                )
                .expect("seed memo relation");
        }
        let canonical = canonical_relation_name("postgres", "public", name);
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&canonical)
            .expect("cardinality stats");
        cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
    }

    #[test]
    fn should_enumerate_three_relation_inner_join_by_cardinality() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_three_relation_order");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        seed_relation(&cassie, "memo_large", 8);
        seed_relation(&cassie, "memo_small", 1);
        seed_relation(&cassie, "memo_medium", 3);
        let session = cassie.create_session("tester", None);

        // Act
        let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT memo_large.item_key FROM memo_large JOIN memo_small ON memo_large.parent_key = memo_small.item_key JOIN memo_medium ON memo_small.parent_key = memo_medium.item_key",
            vec![],
        )
        .expect("explain memo join");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
        assert!(
            plan.contains("join_order=memo_small>memo_medium>memo_large"),
            "plan={plan}"
        );

        let result = cassie
        .execute_sql(
            &session,
            "SELECT memo_large.item_key FROM memo_large JOIN memo_small ON memo_large.parent_key = memo_small.item_key JOIN memo_medium ON memo_small.parent_key = memo_medium.item_key",
            vec![],
        )
        .expect("execute reordered memo join");
        assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_deterministic_fallback_order_when_statistics_are_missing() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_missing_stats_order");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for name in ["zebra", "alpha", "middle"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {name} (id INT)"), vec![])
                .expect("create relation without statistics");
        }

        // Act
        let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT zebra.id FROM zebra JOIN alpha ON zebra.id = alpha.id JOIN middle ON alpha.id = middle.id",
            vec![],
        )
        .expect("explain missing statistics join");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
        assert!(
            plan.contains("join_order=alpha>middle>zebra"),
            "plan={plan}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_keep_schema_qualified_relation_identity_during_join_reordering() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_schema_qualified_identity");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .expect("create reporting schema");
        for statement in [
            "CREATE TABLE public.doc (doc_key INT)",
            "CREATE TABLE reporting.link (pub_key INT, rep_key INT)",
            "CREATE TABLE reporting.doc (doc_key INT)",
            "INSERT INTO public.doc VALUES (1)",
            "INSERT INTO reporting.link VALUES (1, 10), (2, 20)",
            "INSERT INTO reporting.doc VALUES (10), (20), (30)",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect("prepare qualified join fixture");
        }
        for relation in ["public.doc", "reporting.link", "reporting.doc"] {
            let canonical = canonical_relation_name(
                "postgres",
                relation.split('.').next().unwrap(),
                relation.split('.').nth(1).unwrap(),
            );
            let stats = cassie
                .midge
                .rebuild_cardinality_stats_for_collection(&canonical)
                .expect("rebuild relation statistics");
            cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
        }

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT public.doc.doc_key FROM public.doc JOIN reporting.link ON public.doc.doc_key = reporting.link.pub_key JOIN reporting.doc ON reporting.link.rep_key = reporting.doc.doc_key",
            vec![],
        )
        .expect("execute reordered qualified join");

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_outer_join_as_legality_barrier() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_outer_join_barrier");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for name in ["outer_left", "outer_right", "outer_tail"] {
            cassie
                .execute_sql(&session, &format!("CREATE TABLE {name} (id INT)"), vec![])
                .expect("create outer relation");
        }

        // Act
        let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT outer_left.id FROM outer_left LEFT JOIN outer_right ON outer_left.id = outer_right.id JOIN outer_tail ON outer_left.id = outer_tail.id",
            vec![],
        )
        .expect("explain outer join barrier");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("join_enumeration=none"), "plan={plan}");
        assert!(
            plan.contains("join_order=outer_left>outer_right>outer_tail"),
            "plan={plan}"
        );
        assert!(
            plan.contains("join_legality_barriers=left_outer"),
            "plan={plan}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_enumerate_non_equality_inner_joins() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_non_equality_order");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        seed_relation(&cassie, "range_large", 8);
        seed_relation(&cassie, "range_small", 1);
        seed_relation(&cassie, "range_medium", 3);
        let session = cassie.create_session("tester", None);

        // Act
        let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT range_large.item_key FROM range_large JOIN range_small ON range_large.parent_key >= range_small.item_key JOIN range_medium ON range_small.parent_key <= range_medium.item_key",
            vec![],
        )
        .expect("explain non-equality memo join");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
        assert!(
            plan.contains("join_order=range_small>range_medium>range_large"),
            "plan={plan}"
        );
        assert!(
            plan.contains("join_fallback_reason=non_equi_predicate"),
            "plan={plan}"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_use_greedy_expansion_above_eight_relations() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_greedy_nine_relations");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for index in 1..=9 {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE greedy_{index:02} (item_key INT)"),
                    vec![],
                )
                .expect("create greedy relation");
        }
        let mut sql = "EXPLAIN SELECT greedy_09.item_key FROM greedy_09".to_string();
        for index in (1..9).rev() {
            let previous = index + 1;
            write!(
            &mut sql,
            " JOIN greedy_{index:02} ON greedy_{previous:02}.item_key = greedy_{index:02}.item_key"
        )
            .expect("append greedy join");
        }

        // Act
        let explained = cassie
            .execute_sql(&session, &sql, vec![])
            .expect("explain greedy memo join");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("join_enumeration=greedy"), "plan={plan}");
        assert!(
        plan.contains(
            "join_order=greedy_01>greedy_02>greedy_03>greedy_04>greedy_05>greedy_06>greedy_07>greedy_08>greedy_09"
        ),
        "plan={plan}"
    );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_explain_join_physical_properties_with_memory_bound() {
        // Arrange
        use_local_storage();
        let path = data_dir("memo_physical_properties");
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for name in ["property_left", "property_right"] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {name} (item_key INT)"),
                    vec![],
                )
                .expect("create property relation");
        }

        // Act
        let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT property_left.item_key FROM property_left JOIN property_right ON property_left.item_key = property_right.item_key ORDER BY property_left.item_key DESC LIMIT 2",
            vec![],
        )
        .expect("explain join properties");
        let plan = explained.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(
            plan.contains("join_required_columns=property_left.item_key,property_right.item_key"),
            "plan={plan}"
        );
        assert!(
            plan.contains("join_required_ordering=property_left.item_key:desc"),
            "plan={plan}"
        );
        assert!(plan.contains("join_parameterized=false"), "plan={plan}");
        assert!(plan.contains("join_rewindable=true"), "plan={plan}");
        assert!(plan.contains("join_bounded=true"), "plan={plan}");
        assert!(
            plan.contains("join_memory_bound=accounted_query_budget"),
            "plan={plan}"
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/plan_cache.rs.
mod plan_cache {
    use cassie::app::Cassie;
    use cassie::catalog::canonical_relation_name;
    use cassie::config::CassieRuntimeConfig;
    use cassie::runtime::ExecutionMode;
    use cassie::sql::parse_statement;
    use cassie::types::{DataType, FieldSchema, Schema};

    use super::support_metrics as support;
    use support::*;

    fn adaptive_execution_config() -> CassieRuntimeConfig {
        let mut config = CassieRuntimeConfig::default();
        config.limits.adaptive_execution_enabled = true;
        config.limits.adaptive_min_cost_savings_bps = 100;
        config
    }

    #[test]
    fn should_reuse_cached_plan_across_sessions_without_sharing_bind_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("reuse_across_sessions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"title": "beta"}),
                )
                .unwrap();

            let session_one = cassie.create_session("alice", None);
            let session_two = cassie.create_session("bob", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session_one,
                    "SELECT title FROM plan_cache_docs WHERE title = $1",
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session_two,
                    "SELECT title FROM plan_cache_docs WHERE title = $1",
                    vec![cassie::types::Value::String("beta".to_string())],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(
                first.rows[0][0],
                cassie::types::Value::String("alpha".to_string())
            );
            assert_eq!(
                second.rows[0][0],
                cassie::types::Value::String("beta".to_string())
            );
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_report_diagnostic_plan_cache_hit_after_query_execution() {
        // Arrange
        use_local_storage();
        let path = data_dir("diagnostic_cache_hit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_diagnostic_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);
            let sql = "SELECT title FROM plan_cache_diagnostic_docs WHERE title = $1";
            let params = vec![cassie::types::Value::String("alpha".to_string())];
            cassie.execute_sql(&session, sql, params.clone()).unwrap();
            let parsed = parse_statement(sql).unwrap();

            // Act
            let hit = cassie.plan_cache_hit_for_diagnostics(
                &parsed,
                &params,
                ExecutionMode::SimpleQuery,
                session.database.clone(),
                &session.search_path(),
            );

            // Assert
            assert!(hit);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reuse_cached_plan_for_equivalent_sql_with_different_whitespace() {
        // Arrange
        use_local_storage();
        let path = data_dir("reuse_equivalent_sql_whitespace");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_whitespace_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_whitespace_docs WHERE title = $1",
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "  select   title  from   plan_cache_whitespace_docs where   title = $1  ",
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reuse_cached_execution_result_without_additional_storage_reads() {
        // Arrange
        use_local_storage();
        let path = data_dir("execution_result_cache_hit");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "execution_cache_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);
            let before = cassie.metrics();
            let before_reads = before["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default();

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM execution_cache_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let middle = cassie.metrics();
            let middle_reads = middle["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM execution_cache_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let after = cassie.metrics();
            let after_reads = after["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default();

            // Assert
            assert_eq!(first.rows, second.rows);
            assert_eq!(first.rows.len(), 1);
            assert!(middle_reads > before_reads);
            assert_eq!(after_reads, middle_reads);
            assert_eq!(after["query"]["count"].as_u64(), Some(2));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_invalidate_cached_execution_result_after_write() {
        // Arrange
        use_local_storage();
        let path = data_dir("execution_result_cache_invalidation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "execution_cache_invalidation_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM execution_cache_invalidation_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO execution_cache_invalidation_docs (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM execution_cache_invalidation_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 2);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_isolate_cached_plans_by_database() {
        // Arrange
        use_local_storage();
        let path = data_dir("database_isolation");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_database_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let primary = cassie.create_session("alice", Some("primary_db".to_string()));
            let analytics = cassie.create_session("alice", Some("analytics_db".to_string()));
            let sql = "SELECT title FROM plan_cache_database_docs WHERE title = 'alpha'";

            // Act
            let first = cassie.execute_sql(&primary, sql, vec![]).unwrap();
            let second = cassie.execute_sql(&analytics, sql, vec![]).unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(2));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_keep_first_non_durable_plan_miss_out_of_cf2() {
        // Arrange
        use_local_storage();
        let path = data_dir("first_non_durable_miss_out_of_cf2");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_first_miss_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let session = cassie.create_session("alice", None);

            // Act
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_first_miss_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(metrics["storage"]["temp"]["writes"].as_u64(), Some(0));
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_reuse_cf2_cached_plan_after_restart_without_l1_state() {
        // Arrange
        use_local_storage();
        let path = data_dir("reuse_after_restart");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                cassie.startup().unwrap();

                let collection =
                    canonical_relation_name("postgres", "public", "plan_cache_restart_docs");
                let schema = Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                };

                cassie
                    .midge
                    .create_collection(&collection, schema.clone())
                    .unwrap();
                cassie.catalog.register_collection(
                    &collection,
                    schema
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.data_type.clone()))
                        .collect(),
                );
                cassie
                    .ingest_document(&collection, serde_json::json!({"title": "alpha"}))
                    .unwrap();

                let session = cassie.create_session("alice", None);

                let first = cassie
                    .execute_sql(
                        &session,
                        "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                        vec![],
                    )
                    .unwrap();
                let second = cassie
                    .execute_sql(
                        &session,
                        "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                        vec![],
                    )
                    .unwrap();

                assert_eq!(first.rows.len(), 1);
                assert_eq!(second.rows.len(), 1);
                cassie.shutdown();
            }

            let restarted = Cassie::new_with_data_dir(&path).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("alice", None);

            // Act
            let result = restarted
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_restart_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = restarted.metrics();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_separate_cached_plans_by_adaptive_config() {
        // Arrange
        use_local_storage();
        let path = data_dir("adaptive_config_key");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            {
                let cassie = Cassie::new_with_data_dir(&path).unwrap();
                cassie.startup().unwrap();
                let collection = canonical_relation_name(
                    "postgres",
                    "public",
                    "plan_cache_adaptive_config_docs",
                );
                let schema = Schema {
                    fields: vec![FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                };

                cassie
                    .midge
                    .create_collection(&collection, schema.clone())
                    .unwrap();
                cassie.catalog.register_collection(
                    &collection,
                    schema
                        .fields
                        .iter()
                        .map(|field| (field.name.clone(), field.data_type.clone()))
                        .collect(),
                );
                cassie
                    .midge
                    .put_document(
                        &collection,
                        Some("doc-1".to_string()),
                        serde_json::json!({"title": "alpha"}),
                    )
                    .unwrap();

                let session = cassie.create_session("alice", None);
                let sql = "SELECT title FROM plan_cache_adaptive_config_docs WHERE title = 'alpha'";
                cassie.execute_sql(&session, sql, vec![]).unwrap();
                cassie.execute_sql(&session, sql, vec![]).unwrap();
                cassie.shutdown();
            }

            let restarted =
                Cassie::new_with_data_dir_and_config(&path, adaptive_execution_config()).unwrap();
            restarted.startup().unwrap();
            let session = restarted.create_session("alice", None);

            // Act
            let result = restarted
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_adaptive_config_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = restarted.metrics();

            // Assert
            assert_eq!(result.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_invalidate_cached_plan_after_ddl_changes_catalog_state() {
        // Arrange
        use_local_storage();
        let path = data_dir("invalidate_after_ddl");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_ddl_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(&session, "CREATE TABLE plan_cache_guard (id INT)", vec![])
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_ddl_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(2));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_evict_oldest_plan_when_cache_capacity_is_one() {
        // Arrange
        use_local_storage();
        let path = data_dir("eviction");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.plan_cache_entries = 1;
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let collection = "plan_cache_eviction_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-2".to_string()),
                    serde_json::json!({"title": "beta"}),
                )
                .unwrap();

            let session = cassie.create_session("alice", None);

            // Act
            let first = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let second = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_eviction_docs WHERE title = 'beta'",
                    vec![],
                )
                .unwrap();
            let third = cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM plan_cache_eviction_docs WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let metrics = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(third.rows.len(), 1);
            assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(3));
            assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(0));
            assert_eq!(metrics["plan_cache"]["evictions"].as_u64(), Some(2));

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_not_cache_transaction_control_statements() {
        // Arrange
        use_local_storage();
        let path = data_dir("transaction_controls_bypass_cache");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("alice", None);
            let before = cassie.metrics();

            // Act
            cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
            cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                after["plan_cache"]["hits"].as_u64(),
                before["plan_cache"]["hits"].as_u64()
            );
            assert_eq!(
                after["plan_cache"]["misses"].as_u64(),
                before["plan_cache"]["misses"].as_u64()
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_promote_non_durable_l1_plan_without_extra_cf2_reads_on_second_hit() {
        // Arrange
        use_local_storage();
        let path = data_dir("l1_promotion_without_extra_cf2_reads");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            let collection = "plan_cache_l1_promotion_docs";
            let schema = Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            };

            cassie
                .midge
                .create_collection(collection, schema.clone())
                .unwrap();
            cassie.catalog.register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
            cassie
                .midge
                .put_document(
                    collection,
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let session = cassie.create_session("alice", None);

            let first_sql = "SELECT title FROM plan_cache_l1_promotion_docs WHERE title = 'alpha'";
            // Act
            let first = cassie.execute_sql(&session, first_sql, vec![]).unwrap();
            let after_first = cassie.metrics();
            let second = cassie.execute_sql(&session, first_sql, vec![]).unwrap();
            let after_second = cassie.metrics();

            // Assert
            assert_eq!(first.rows.len(), 1);
            assert_eq!(second.rows.len(), 1);
            assert_eq!(
                after_second["storage"]["temp"]["reads"].as_u64(),
                after_first["storage"]["temp"]["reads"].as_u64()
            );
            let writes_after_first = after_first["storage"]["temp"]["writes"]
                .as_u64()
                .expect("first temp writes");
            let writes_after_second = after_second["storage"]["temp"]["writes"]
                .as_u64()
                .expect("second temp writes");
            assert_eq!(writes_after_second.saturating_sub(writes_after_first), 1);

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/planner_aggregates_sets.rs.
mod planner_aggregates_sets {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ];

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_plan_grouped_distinct_select_controls() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_grouped");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_grouped GROUP BY title HAVING COUNT(*) > 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert!(logical.distinct);
        assert_eq!(logical.group_by.len(), 1);
        assert!(logical.having.is_some());
    });
    }

    #[test]
    fn should_keep_set_query_result_controls_in_logical_plan() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_set_result_left");
        register_test_collection(&catalog, "planner_set_result_right");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_set_result_left UNION ALL SELECT title FROM planner_set_result_right ORDER BY title DESC LIMIT 2 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();

        // Act
        let logical = logical::plan(&bound).unwrap();

        // Assert
        assert!(logical.set.is_some());
        assert_eq!(logical.order.len(), 1);
        assert!(matches!(logical.order[0].direction, SortDirection::Desc));
        assert_eq!(logical.limit, Some(2));
        assert_eq!(logical.offset, Some(1));
    });
    }

    #[test]
    fn should_build_physical_operators_for_aggregate_distinct_set() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_set_left");
        register_test_collection(&catalog, "planner_set_right");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_set_left GROUP BY title UNION SELECT title, COUNT(*) AS total FROM planner_set_right GROUP BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Aggregate)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Distinct)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::SetOperation)));
    });
    }

    #[test]
    fn should_mark_simple_aggregate_as_parallel_candidate() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_parallel_aggregate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title, COUNT(*) AS total FROM planner_parallel_aggregate GROUP BY title",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert!(physical_plan
                .operators
                .iter()
                .any(|operator| matches!(operator, Operator::Aggregate)));
            assert!(physical_plan.aggregate.parallel_candidate);
        });
    }

    #[test]
    fn should_keep_distinct_aggregate_on_serial_candidate_path() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_serial_distinct_aggregate");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT DISTINCT title, COUNT(*) AS total FROM planner_serial_distinct_aggregate GROUP BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::Aggregate)));
        assert!(!physical_plan.aggregate.parallel_candidate);
    });
    }
}

// Formerly tests/planner_commands.rs.
mod planner_commands {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ];

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_plan_create_table_as_command() {
        // Arrange
        let catalog = Catalog::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement(
                "CREATE TABLE planner_create (id INT, title TEXT, body TEXT)",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_create");
            assert!(plan.command.is_some());
            assert!(plan.projection.is_empty());
            assert!(plan.filter.is_none());
        });
    }

    #[test]
    fn should_plan_drop_table_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_drop");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement("DROP TABLE planner_drop").unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_drop");
            assert!(plan.command.is_some());
        });
    }

    #[test]
    fn should_plan_insert_values_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_insert_values");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement(
                "INSERT INTO planner_insert_values (title) VALUES ('alpha')",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_insert_values");
            match plan.command.as_ref().expect("insert command") {
                logical::LogicalCommand::Insert(statement) => {
                    assert_eq!(statement.table, "planner_insert_values");
                    assert_eq!(statement.columns, vec!["title".to_string()]);
                    assert!(matches!(statement.source, InsertSource::Values(_)));
                }
                _ => panic!("expected insert command"),
            }
        });
    }

    #[test]
    fn should_plan_insert_select_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_insert_select_target");
        register_test_collection(&catalog, "planner_insert_select_source");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        // Act
        let parsed = parser::parse_statement(
            "INSERT INTO planner_insert_select_target (title) SELECT title FROM planner_insert_select_source",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let plan = logical::plan(&bound).unwrap();

        // Assert
        assert_eq!(plan.collection, "planner_insert_select_target");
        match plan.command.as_ref().expect("insert command") {
            logical::LogicalCommand::Insert(statement) => {
                assert_eq!(statement.table, "planner_insert_select_target");
                assert_eq!(statement.columns, vec!["title".to_string()]);
                assert!(matches!(statement.source, InsertSource::Select(_)));
            }
            _ => panic!("expected insert command"),
        }
    });
    }

    #[test]
    fn should_plan_update_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_update");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement(
                "UPDATE planner_update SET title = 'alpha' WHERE body = 'old' RETURNING title",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_update");
            match plan.command.as_ref().expect("update command") {
                logical::LogicalCommand::Update(statement) => {
                    assert_eq!(statement.table, "planner_update");
                    assert_eq!(statement.assignments.len(), 1);
                    assert!(statement.filter.is_some());
                    assert_eq!(statement.returning.len(), 1);
                }
                _ => panic!("expected update command"),
            }
        });
    }

    #[test]
    fn should_plan_delete_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_delete");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement(
                "DELETE FROM planner_delete WHERE title = 'alpha' RETURNING title",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_delete");
            match plan.command.as_ref().expect("delete command") {
                logical::LogicalCommand::Delete(statement) => {
                    assert_eq!(statement.table, "planner_delete");
                    assert!(statement.filter.is_some());
                    assert_eq!(statement.returning.len(), 1);
                }
                _ => panic!("expected delete command"),
            }
        });
    }

    #[test]
    fn should_plan_alter_table_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_alter");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed =
                parser::parse_statement("ALTER TABLE planner_alter ADD COLUMN score FLOAT")
                    .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_alter");
            assert!(plan.command.is_some());
            match plan.command.as_ref().unwrap() {
                logical::LogicalCommand::AlterTable(statement) => {
                    assert_eq!(statement.table, "planner_alter");
                }
                _ => panic!("expected alter table command"),
            }
        });
    }

    #[test]
    fn should_plan_create_index_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_create_index");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parser::parse_statement(
                "CREATE UNIQUE INDEX planner_idx_title ON planner_create_index USING btree (title)",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_create_index");
            assert!(plan.command.is_some());
            match plan.command.as_ref().unwrap() {
                logical::LogicalCommand::CreateIndex(statement) => {
                    assert_eq!(statement.name, "planner_idx_title");
                    assert!(statement.unique);
                }
                _ => panic!("expected create index command"),
            }
        });
    }

    #[test]
    fn should_plan_drop_index_as_command() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_drop_index");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_index(cassie::catalog::IndexMeta {
                collection: "planner_drop_index".to_string(),
                name: "planner_idx_title".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: cassie::catalog::IndexKind::Scalar,
                unique: false,
                options: std::collections::BTreeMap::new(),
            });

            // Act
            let parsed =
                parser::parse_statement("DROP INDEX planner_idx_title ON planner_drop_index")
                    .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(plan.collection, "planner_drop_index");
            assert!(plan.command.is_some());
            match plan.command.as_ref().unwrap() {
                logical::LogicalCommand::DropIndex(statement) => {
                    assert_eq!(statement.name, "planner_idx_title");
                    assert_eq!(statement.table, "planner_drop_index");
                }
                _ => panic!("expected drop index command"),
            }
        });
    }
}

// Formerly tests/planner_costs.rs.
mod planner_costs {
    #![allow(unused_imports, dead_code)]

    use cassie::app::Cassie;

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_explain_cost_model_diagnostics_for_index_choice() {
        // Arrange
        use_local_storage();
        let path = data_dir("planner_cost_diagnostics");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE planner_cost_docs (tenant TEXT, title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_planner_cost_tenant ON planner_cost_docs USING btree (tenant)",
                vec![],
            )
            .unwrap();

            // Act
            let explained = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM planner_cost_docs WHERE tenant = 'acme'",
                    vec![],
                )
                .unwrap();

            // Assert
            let plan = match &explained.rows[0][0] {
                cassie::types::Value::String(value) => value,
                other => panic!("expected explain string, got {other:?}"),
            };
            assert!(plan.contains("cost_model=v2"));
            assert!(plan.contains("selected_cost="));
            assert!(plan.contains("cost_source="));
            assert!(plan.contains("rejected_alternatives="));

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/planner_estimates.rs.
mod planner_estimates {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ];

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_fallback_to_conservative_cardinality_estimates_when_stats_missing() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_cardinality_fallback");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_cardinality_fallback WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);
            let cardinality_stats = std::collections::HashMap::new();

            // Act
            let physical_plan =
                physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(physical_plan.estimates.scan_rows, 1_000);
            assert_eq!(physical_plan.estimates.index_rows, 1_000);
        });
    }

    #[test]
    fn should_use_hydrated_cardinality_estimates_for_index_plans() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_cardinality_hydrated");
        catalog.hydrate_cardinality_stats(
            "planner_cardinality_hydrated",
            cassie::catalog::CollectionCardinalityStats {
                built_generation: 0,
                row_count: 42,
                hydrated: true,
                indexes: std::collections::BTreeMap::from([(
                    "scalar:planner_cardinality_hydrated_title_idx".to_string(),
                    cassie::catalog::IndexCardinalityStats { cardinality: 7 },
                )]),
                fields: BTreeMap::default(),
            },
        );
        catalog.register_index(cassie::catalog::IndexMeta {
            collection: "planner_cardinality_hydrated".to_string(),
            name: "planner_cardinality_hydrated_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: cassie::catalog::IndexKind::Scalar,
            unique: false,
            options: BTreeMap::default(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT body FROM planner_cardinality_hydrated WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);
            let cardinality_stats = catalog.cardinality_snapshot();

            // Act
            let physical_plan =
                physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(physical_plan.estimates.scan_rows, 42);
            assert_eq!(physical_plan.estimates.index_rows, 7);
        });
    }

    #[test]
    fn should_use_advanced_field_stats_for_selective_filter_estimates() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_cardinality_advanced");
        register_scalar_index(
            &catalog,
            "planner_cardinality_advanced",
            "planner_cardinality_advanced_title_idx",
            vec!["title"],
        );
        catalog.hydrate_cardinality_stats(
            "planner_cardinality_advanced",
            cassie::catalog::CollectionCardinalityStats {
                built_generation: 0,
                row_count: 100,
                hydrated: true,
                indexes: std::collections::BTreeMap::from([(
                    "scalar:planner_cardinality_advanced_title_idx".to_string(),
                    cassie::catalog::IndexCardinalityStats { cardinality: 80 },
                )]),
                fields: std::collections::BTreeMap::from([(
                    "title".to_string(),
                    cassie::catalog::FieldCardinalityStats {
                        non_null_count: 100,
                        distinct_count: 3,
                        sample_count: 100,
                        confidence: 100,
                        histogram_buckets: vec![cassie::catalog::FieldHistogramBucket {
                            lower: "\"alpha\"".to_string(),
                            upper: "\"gamma\"".to_string(),
                            count: 100,
                        }],
                        heavy_hitters: vec![cassie::catalog::FieldHeavyHitter {
                            value: "\"alpha\"".to_string(),
                            count: 5,
                        }],
                        ..Default::default()
                    },
                )]),
            },
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT body FROM planner_cardinality_advanced WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);
            let cardinality_stats = catalog.cardinality_snapshot();

            // Act
            let physical_plan =
                physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(physical_plan.estimates.scan_rows, 100);
            assert_eq!(physical_plan.estimates.index_rows, 5);
            assert_eq!(physical_plan.estimates.cost_source, "advanced_stats");
        });
    }

    #[test]
    fn should_choose_lower_cost_competing_scalar_index_from_stats() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_competing_indexes");
        register_scalar_index(
            &catalog,
            "planner_competing_indexes",
            "planner_competing_title_idx",
            vec!["title"],
        );
        register_scalar_index(
            &catalog,
            "planner_competing_indexes",
            "planner_competing_body_idx",
            vec!["body"],
        );
        catalog.hydrate_cardinality_stats(
            "planner_competing_indexes",
            cassie::catalog::CollectionCardinalityStats {
                built_generation: 0,
                row_count: 100,
                hydrated: true,
                indexes: std::collections::BTreeMap::from([
                    (
                        "scalar:planner_competing_title_idx".to_string(),
                        cassie::catalog::IndexCardinalityStats { cardinality: 80 },
                    ),
                    (
                        "scalar:planner_competing_body_idx".to_string(),
                        cassie::catalog::IndexCardinalityStats { cardinality: 5 },
                    ),
                ]),
                fields: BTreeMap::default(),
            },
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
            "SELECT title FROM planner_competing_indexes WHERE title = 'alpha' AND body = 'one'",
        )
        .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);
            let cardinality_stats = catalog.cardinality_snapshot();

            // Act
            let physical_plan =
                physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_competing_body_idx")
            );
            assert_eq!(physical_plan.estimates.index_rows, 5);
        });
    }
}

// Formerly tests/planner_indexes.rs.
mod planner_indexes {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ];

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_select_scalar_index_for_equality_filter() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_index_aware");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_index(cassie::catalog::IndexMeta {
                collection: "planner_index_aware".to_string(),
                name: "planner_index_aware_title_idx".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: cassie::catalog::IndexKind::Scalar,
                unique: false,
                options: std::collections::BTreeMap::default(),
            });
            let parsed = parser::parse_statement(
                "SELECT body FROM planner_index_aware WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let cardinality_stats = std::collections::HashMap::new();
            let physical_plan =
                physical::build_with_indexes(logical, bound.indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_index_aware_title_idx")
            );
        });
    }

    #[test]
    fn should_mark_scalar_index_plan_as_covered() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_covering_index");
        register_scalar_index(
            &catalog,
            "planner_covering_index",
            "planner_covering_title_idx",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_covering_index WHERE title = 'alpha' ORDER BY title",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_covering_index");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_covering_title_idx")
            );
            assert!(physical_plan.read.covered_index);
        });
    }

    #[test]
    fn should_leave_noncovered_scalar_index_plan_uncovered() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_covering_fallback");
        register_scalar_index(
            &catalog,
            "planner_covering_fallback",
            "planner_covering_fallback_title_idx",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT body FROM planner_covering_fallback WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_covering_fallback");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_covering_fallback_title_idx")
            );
            assert!(!physical_plan.read.covered_index);
        });
    }

    #[test]
    fn should_mark_include_column_plan_as_covered() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_include_covering");
        catalog.register_index(IndexMeta {
            collection: "planner_include_covering".to_string(),
            name: "planner_include_covering_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: vec!["body".to_string()],
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT body FROM planner_include_covering WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_include_covering");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_include_covering_title_idx")
            );
            assert!(physical_plan.read.covered_index);
        });
    }

    #[test]
    fn should_select_partial_index_for_exact_predicate() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_partial_index");
        let predicate = parser::parse_statement(
            "SELECT title FROM planner_partial_index WHERE title = 'alpha'",
        )
        .and_then(|parsed| match parsed.statement {
            cassie::sql::ast::QueryStatement::Select(select) => Ok(select.filter.expect("filter")),
            _ => unreachable!(),
        })
        .unwrap();
        catalog.register_index(IndexMeta {
            collection: "planner_partial_index".to_string(),
            name: "planner_partial_index_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: Some(serde_json::to_string(&predicate).unwrap()),
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_partial_index WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_partial_index");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_partial_index_title_idx")
            );
        });
    }

    #[test]
    fn should_skip_partial_index_for_unsafe_predicate() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_partial_fallback");
        catalog.register_index(IndexMeta {
            collection: "planner_partial_fallback".to_string(),
            name: "planner_partial_fallback_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: Some("{\"Column\":\"status\"}".to_string()),
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_partial_fallback WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_partial_fallback");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert!(physical_plan.read.selected_index.is_none());
        });
    }

    #[test]
    fn should_select_expression_index_for_matching_predicate() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_expression_index");
        let expression = parser::parse_statement(
        "CREATE INDEX planner_expression_index_lower_idx ON planner_expression_index USING btree (lower(title))",
    )
    .and_then(|parsed| match parsed.statement {
        QueryStatement::CreateIndex(statement) => Ok(statement.expressions[0].clone()),
        _ => unreachable!(),
    })
    .unwrap();
        catalog.register_index(IndexMeta {
            collection: "planner_expression_index".to_string(),
            name: "planner_expression_index_lower_idx".to_string(),
            field: String::new(),
            fields: Vec::new(),
            expressions: vec![serde_json::to_string(&expression).unwrap()],
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_expression_index WHERE lower(title) = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_expression_index");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_expression_index_lower_idx")
            );
        });
    }

    #[test]
    fn should_skip_expression_index_for_non_equivalent_predicate() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_expression_fallback");
        let expression = parser::parse_statement(
        "CREATE INDEX planner_expression_fallback_lower_idx ON planner_expression_fallback USING btree (lower(title))",
    )
    .and_then(|parsed| match parsed.statement {
        QueryStatement::CreateIndex(statement) => Ok(statement.expressions[0].clone()),
        _ => unreachable!(),
    })
    .unwrap();
        catalog.register_index(IndexMeta {
            collection: "planner_expression_fallback".to_string(),
            name: "planner_expression_fallback_lower_idx".to_string(),
            field: String::new(),
            fields: Vec::new(),
            expressions: vec![serde_json::to_string(&expression).unwrap()],
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_expression_fallback WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_expression_fallback");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert!(physical_plan.read.selected_index.is_none());
        });
    }

    #[test]
    fn should_keep_scalar_index_selection_deterministic_when_candidates_tie() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_operator_feedback_tie");
        register_scalar_index(
            &catalog,
            "planner_operator_feedback_tie",
            "planner_operator_feedback_body_idx_a",
            vec!["body"],
        );
        register_scalar_index(
            &catalog,
            "planner_operator_feedback_tie",
            "planner_operator_feedback_title_idx_b",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_operator_feedback_tie WHERE title = 'alpha' AND body = 'one'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let indexes = catalog.list_indexes("planner_operator_feedback_tie");
        let cardinality_stats =
            std::collections::HashMap::<String, cassie::catalog::CollectionCardinalityStats>::new();
        let physical_plan =
            physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_operator_feedback_body_idx_a")
        );
    });
    }
}

// Formerly tests/planner_logical.rs.
mod planner_logical {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, CollectionStorageMode, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        let schema = vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ];

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_plan_select_collection_projection_filter_limit_offset() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_projection");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title, body FROM planner_projection WHERE title = 'alpha' ORDER BY title DESC LIMIT 2 OFFSET 1",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let plan = logical::plan(&bound).unwrap();

            // Act
            // assertions are direct on the logical plan shape

            // Assert
            assert_eq!(plan.collection, "planner_projection");
            assert_eq!(plan.projection.len(), 2);
            assert!(matches!(
                &plan.projection[0],
                SelectItem::Column { name, alias } if name == "title" && alias.is_none()
            ));
            assert!(matches!(
                &plan.projection[1],
                SelectItem::Column { name, alias } if name == "body" && alias.is_none()
            ));
            assert!(plan.filter.is_some());
            assert_eq!(plan.order.len(), 1);
            assert!(matches!(
                &plan.order[0].expr,
                Expr::Column(field) if field == "title"
            ));
            assert!(matches!(plan.order[0].direction, SortDirection::Desc));
            assert_eq!(plan.limit, Some(2));
            assert_eq!(plan.offset, Some(1));
        });
    }

    #[test]
    fn should_preserve_offset_shape_in_optimizer() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_offset_shape");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let plans = [
                "SELECT title FROM planner_offset_shape ORDER BY title ASC LIMIT 3",
                "SELECT title FROM planner_offset_shape ORDER BY title ASC LIMIT 3 OFFSET 500",
            ]
            .map(|sql| {
                let parsed = parser::parse_statement(sql).unwrap();
                let bound = binder::bind(parsed, &catalog).unwrap();
                logical::plan(&bound).unwrap()
            });

            // Act
            let [without_offset, with_offset] = plans.map(optimizer::optimize);

            // Assert
            assert_eq!(without_offset.offset, None);
            assert_eq!(with_offset.offset, Some(500));
        });
    }

    #[test]
    fn should_omit_offset_node_without_clause() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_default_offset_operator");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT id FROM planner_default_offset_operator ORDER BY id ASC LIMIT 5",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let optimized = optimizer::optimize(logical);
            let physical_plan = physical::build(optimized);

            // Assert
            assert_eq!(physical_plan.operators.len(), 4);
            assert!(matches!(
                physical_plan.operators.get(3),
                Some(Operator::Limit)
            ));
            assert!(!physical_plan.operators.contains(&Operator::Offset));
        });
    }

    #[test]
    fn should_preserve_column_store_storage_mode_in_create_table_logical_command() {
        // Arrange
        let catalog = Catalog::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "CREATE TABLE planner_column_store_docs (id TEXT, title TEXT) WITH (storage = column_store)",
            )
            .expect("parse should succeed");
            let bound = binder::bind(parsed, &catalog).expect("bind should succeed");

            // Act
            let logical = logical::plan(&bound).expect("logical plan");

            // Assert
            let Some(cassie::planner::logical::LogicalCommand::CreateTable(statement)) = logical.command
            else {
                panic!("expected create table logical command");
            };
            assert_eq!(statement.table, "planner_column_store_docs");
            assert_eq!(statement.storage_mode, CollectionStorageMode::ColumnStore);
        });
    }

    #[test]
    fn should_keep_collection_clause_values_in_logical_plan() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_clauses");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT * FROM planner_clauses WHERE body = 'hello' ORDER BY title ASC LIMIT 1 OFFSET 2",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();

            // Act
            let logical = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(logical.collection, "planner_clauses");
            assert!(matches!(&logical.projection[..], [SelectItem::Wildcard]));
            assert_eq!(logical.limit, Some(1));
            assert_eq!(logical.offset, Some(2));
            assert_eq!(logical.order.len(), 1);

            match logical.filter.as_ref().expect("filter should exist") {
                Expr::Binary { left, right, op } => {
                    assert!(matches!(op, BinaryOp::Eq));
                    assert!(matches!(left.as_ref(), Expr::Column(name) if name == "body"));
                    assert!(matches!(right.as_ref(), Expr::StringLiteral(query) if query == "hello"));
                }
                _ => panic!("expected filter expression"),
            }

            assert!(matches!(
                &logical.order[0].expr,
                Expr::Column(field) if field == "title"
            ));
            assert!(matches!(logical.order[0].direction, SortDirection::Asc));
        });
    }

    #[test]
    fn should_reject_invalid_logical_plan_shape_missing_collection() {
        // Arrange
        let bound = BoundStatement {
            statement: ParsedStatement {
                raw_sql: "SELECT id FROM  LIMIT 1".to_string(),
                statement: QueryStatement::Select(SelectStatement {
                    source: QuerySource::Collection(String::new()),
                    ctes: vec![],
                    recursive: false,
                    distinct: false,
                    distinct_on: Vec::new(),
                    projection: vec![SelectItem::Column {
                        name: "id".to_string(),
                        alias: None,
                    }],
                    filter: None,
                    group_by: vec![],
                    having: None,
                    order: vec![],
                    limit: Some(1),
                    offset: Some(0),
                    set: None,
                }),
            },
            indexes: Vec::new(),
        };

        // Act
        let result = logical::plan(&bound);

        // Assert
        let error = result.unwrap_err();
        match error {
            CassieError::Planner(message) => assert!(message.contains("source")),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn should_reject_invalid_logical_plan_shape_empty_projection() {
        // Arrange
        let bound = BoundStatement {
            statement: ParsedStatement {
                raw_sql: "SELECT FROM planner_projectionless".to_string(),
                statement: QueryStatement::Select(SelectStatement {
                    source: QuerySource::Collection("planner_projectionless".to_string()),
                    ctes: vec![],
                    recursive: false,
                    distinct: false,
                    distinct_on: Vec::new(),
                    projection: vec![],
                    filter: None,
                    group_by: vec![],
                    having: None,
                    order: vec![],
                    limit: Some(1),
                    offset: Some(0),
                    set: None,
                }),
            },
            indexes: Vec::new(),
        };

        // Act
        let result = logical::plan(&bound);

        // Assert
        let error = result.unwrap_err();
        match error {
            CassieError::Planner(message) => assert!(message.contains("projection")),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn should_reject_invalid_logical_plan_shape_negative_offset() {
        // Arrange
        let bound = BoundStatement {
            statement: ParsedStatement {
                raw_sql: "SELECT id FROM planner_negative_offset".to_string(),
                statement: QueryStatement::Select(SelectStatement {
                    source: QuerySource::Collection("planner_negative_offset".to_string()),
                    ctes: vec![],
                    recursive: false,
                    distinct: false,
                    distinct_on: Vec::new(),
                    projection: vec![SelectItem::Column {
                        name: "id".to_string(),
                        alias: None,
                    }],
                    filter: None,
                    group_by: vec![],
                    having: None,
                    order: vec![],
                    limit: Some(10),
                    offset: Some(-1),
                    set: None,
                }),
            },
            indexes: Vec::new(),
        };

        // Act
        let result = logical::plan(&bound);

        // Assert
        let error = result.unwrap_err();
        match error {
            CassieError::Planner(message) => assert!(message.contains("offset")),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn should_reject_invalid_logical_plan_shape_negative_limit() {
        // Arrange
        let bound = BoundStatement {
            statement: ParsedStatement {
                raw_sql: "SELECT id FROM planner_negative_limit".to_string(),
                statement: QueryStatement::Select(SelectStatement {
                    source: QuerySource::Collection("planner_negative_limit".to_string()),
                    ctes: vec![],
                    recursive: false,
                    distinct: false,
                    distinct_on: Vec::new(),
                    projection: vec![SelectItem::Column {
                        name: "id".to_string(),
                        alias: None,
                    }],
                    filter: None,
                    group_by: vec![],
                    having: None,
                    order: vec![],
                    limit: Some(-10),
                    offset: Some(0),
                    set: None,
                }),
            },
            indexes: Vec::new(),
        };

        // Act
        let result = logical::plan(&bound);

        // Assert
        let error = result.unwrap_err();
        match error {
            CassieError::Planner(message) => assert!(message.contains("limit")),
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn should_be_deterministic_for_repeated_planning_of_same_query() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_repeat_logical");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_repeat_logical WHERE title = 'gamma' ORDER BY title ASC LIMIT 3 OFFSET 1",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();

            // Act
            let first = logical::plan(&bound).unwrap();
            let second = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(format!("{first:?}"), format!("{:?}", second));
        });
    }

    #[test]
    fn should_be_deterministic_for_repeated_optimization_of_same_logical_plan() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_repeat_optimizer");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_repeat_optimizer WHERE title = 'gamma' ORDER BY title ASC LIMIT 3",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical_plan = logical::plan(&bound).unwrap();

            // Act
            let first = optimizer::optimize(logical_plan.clone());
            let second = optimizer::optimize(logical_plan);

            // Assert
            assert_eq!(format!("{first:?}"), format!("{:?}", second));
            assert_eq!(first.offset, None);
            assert_eq!(second.offset, None);
        });
    }

    #[test]
    fn should_plan_non_recursive_cte_source_as_logical_source() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_cte_source");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "WITH docs_cte AS (SELECT title FROM planner_cte_source) SELECT title FROM docs_cte",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();

            // Act
            let logical = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(logical.ctes.len(), 1);
            assert_eq!(logical.source, QuerySource::Cte("docs_cte".to_string()));
            assert_eq!(logical.collection, "docs_cte");
            assert_eq!(logical.ctes[0].name, "docs_cte");
        });
    }

    #[test]
    fn should_preserve_recursive_cte_aliases_in_logical_plan() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_recursive_aliases");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "WITH RECURSIVE seq(id) AS (SELECT id FROM planner_recursive_aliases UNION ALL SELECT id FROM seq) SELECT id FROM seq",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();

            // Act
            let logical = logical::plan(&bound).unwrap();

            // Assert
            assert_eq!(logical.ctes.len(), 1);
            assert_eq!(logical.ctes[0].aliases, vec!["id".to_string()]);
            assert_eq!(logical.ctes[0].name, "seq");
            let recursive = matches!(
                logical.ctes[0].query,
                cassie::sql::ast::CteQuery::Recursive { .. }
            );
            assert!(recursive);
        });
    }
}
// Formerly tests/planner_physical.rs.
mod planner_physical {
    #![allow(unused_imports, dead_code)]
    use cassie::app::CassieError;
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, optimizer, physical, physical::Operator};
    use cassie::sql::ast::{
        BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
        SelectItem, SelectStatement, SortDirection,
    };
    use cassie::sql::binder::BoundStatement;
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_test_collection(catalog: &Catalog, name: &str) {
        register_collection_fields(
            catalog,
            name,
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
    }

    fn register_collection_fields(catalog: &Catalog, name: &str, schema: Vec<FieldSchema>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    #[test]
    fn should_build_physical_operators_in_execution_order() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_physical");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_physical WHERE title = 'alpha' ORDER BY title DESC LIMIT 2 OFFSET 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.operators.len(), 6);
        assert!(matches!(
            physical_plan.operators.first(),
            Some(Operator::Scan)
        ));
        assert!(matches!(
            physical_plan.operators.get(1),
            Some(Operator::Filter)
        ));
        assert!(matches!(
            physical_plan.operators.get(2),
            Some(Operator::Sort)
        ));
        assert!(matches!(
            physical_plan.operators.get(3),
            Some(Operator::Project)
        ));
        assert!(matches!(
            physical_plan.operators.get(4),
            Some(Operator::Offset)
        ));
        assert!(matches!(
            physical_plan.operators.get(5),
            Some(Operator::Limit)
        ));
    });
    }

    #[test]
    fn should_keep_scan_operator_for_parallel_scan_candidates() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_parallel_scan");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement("SELECT title FROM planner_parallel_scan")
                .expect("parse should succeed");
            let bound = binder::bind(parsed, &catalog).expect("bind should succeed");
            let logical = logical::plan(&bound).expect("logical plan");

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(physical_plan.operators.len(), 2);
            assert!(matches!(physical_plan.operators[0], Operator::Scan));
            assert!(matches!(physical_plan.operators[1], Operator::Project));
        });
    }

    #[test]
    fn should_keep_fixed_plan_adaptive_diagnostics_disabled() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_adaptive_fixed");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_adaptive_fixed WHERE title = 'alpha'",
            )
            .expect("parse should succeed");
            let bound = binder::bind(parsed, &catalog).expect("bind should succeed");
            let logical = logical::plan(&bound).expect("logical plan");
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert!(!physical_plan.adaptive_plan.enabled);
            assert!(physical_plan.adaptive_plan.decision_point.is_empty());
            assert!(physical_plan.adaptive_plan.candidates.is_empty());
        });
    }

    #[test]
    fn should_mark_literal_equality_filter_as_pushed_down() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_predicate_pushdown");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_predicate_pushdown WHERE title = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert!(physical_plan.read.predicate_pushdown);
        });
    }

    #[test]
    fn should_mark_projected_scan_fields_for_projection_pruning() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_projection_pruning");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_projection_pruning WHERE body = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(
                physical_plan.read.projected_scan_fields,
                vec!["title".to_string(), "body".to_string()]
            );
        });
    }

    #[test]
    fn should_mark_scan_limit_for_limit_pushdown() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_limit_pushdown");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_limit_pushdown LIMIT 20 OFFSET 5",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(physical_plan.read.scan_limit, Some(25));
        });
    }

    #[test]
    fn should_mark_order_limit_query_as_top_k() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_top_k");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_top_k ORDER BY title DESC LIMIT 5",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert!(physical_plan.top_k.enabled);
            assert_eq!(physical_plan.top_k.limit, Some(5));
            assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
            assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
        });
    }

    #[test]
    fn should_plan_join_source_with_physical_join_operator() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_join_left");
        register_test_collection(&catalog, "planner_join_right");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT planner_join_left.title FROM planner_join_left JOIN planner_join_right ON planner_join_left.title = planner_join_right.title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.collection, "join");
        assert!(matches!(
            physical_plan.logical.source,
            QuerySource::Join {
                kind: JoinKind::Inner,
                ..
            }
        ));
        assert!(matches!(
            physical_plan.operators.get(1),
            Some(Operator::Join)
        ));
        assert_eq!(physical_plan.join.strategy.as_deref(), Some("hash"));
        assert!(physical_plan.join.vectorized.candidate);
        assert_eq!(
            physical_plan.join.vectorized.fallback_reason.as_deref(),
            None
        );
    });
    }

    #[test]
    fn should_mark_row_id_filter_as_point_lookup_access_path() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_point_lookup");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT id, title FROM planner_point_lookup WHERE id = 'alpha'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(
                physical_plan.read.access_path,
                physical::ReadAccessPath::PointLookup
            );
            assert_eq!(physical_plan.read.access_path_reason, "point-lookup-id");
            assert_eq!(physical_plan.read.fallback_reason, None);
            assert_eq!(
                physical_plan.read.pagination_strategy,
                physical::PaginationStrategy::None
            );
            assert_eq!(physical_plan.top_k.mode, physical::TopKMode::None);
            assert_eq!(
                physical_plan.read.early_stop,
                physical::EarlyStopMode::PointLookup
            );
            assert_eq!(
                physical_plan.projection.shape,
                physical::ProjectionShape::MaterializedProjection
            );
        });
    }

    #[test]
    fn should_mark_row_id_order_limit_as_storage_top_k() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_row_id_top_k");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT id, title FROM planner_row_id_top_k ORDER BY id ASC LIMIT 5",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(
                physical_plan.read.access_path,
                physical::ReadAccessPath::CollectionScan
            );
            assert_eq!(physical_plan.read.access_path_reason, "row-key-top-k");
            assert_eq!(
                physical_plan.read.pagination_strategy,
                physical::PaginationStrategy::Limit
            );
            assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Storage);
            assert_eq!(
                physical_plan.read.early_stop,
                physical::EarlyStopMode::StorageTopK
            );
        });
    }

    #[test]
    fn should_mark_row_id_cursor_as_keyset_pagination() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_row_id_keyset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT id, title FROM planner_row_id_keyset WHERE id > 'cursor' ORDER BY id ASC LIMIT 5",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.read.access_path_reason, "row-key-keyset");
        assert_eq!(
            physical_plan.read.pagination_strategy,
            physical::PaginationStrategy::Keyset
        );
        assert_eq!(physical_plan.top_k.mode, physical::TopKMode::None);
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::Keyset);
    });
    }

    #[test]
    fn should_mark_row_id_offset_page_as_degraded_offset() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_row_id_offset_page");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT id, title FROM planner_row_id_offset_page ORDER BY id ASC LIMIT 5 OFFSET 2",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(
                physical_plan.read.access_path_reason,
                "row-key-ordered-page"
            );
            assert_eq!(
                physical_plan.read.fallback_reason,
                Some("offset-degraded".to_string())
            );
            assert_eq!(
                physical_plan.read.pagination_strategy,
                physical::PaginationStrategy::DegradedOffset
            );
            assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
        });
    }

    #[test]
    fn should_fallback_from_point_lookup_to_collection_scan_with_offset() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_point_lookup_offset");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT id, title FROM planner_point_lookup_offset WHERE id = 'alpha' OFFSET 1",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let logical = optimizer::optimize(logical);

            // Act
            let physical_plan = physical::build(logical);

            // Assert
            assert_eq!(
                physical_plan.read.access_path,
                physical::ReadAccessPath::CollectionScan
            );
            assert_eq!(
                physical_plan.read.fallback_reason,
                Some("offset-degraded".to_string())
            );
            assert_eq!(
                physical_plan.read.pagination_strategy,
                physical::PaginationStrategy::Offset
            );
        });
    }

    #[test]
    fn should_mark_composite_equality_as_prefix_scan() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_prefix_scan",
            vec![
                FieldSchema {
                    name: "tenant_id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_prefix_scan",
            "planner_prefix_scan_tenant_status_idx",
            vec!["tenant_id", "status"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_prefix_scan WHERE tenant_id = 'tenant-a' AND status = 'open'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let indexes = catalog.list_indexes("planner_prefix_scan");
        let cardinality_stats =
            std::collections::HashMap::<String, cassie::catalog::CollectionCardinalityStats>::new();
        let physical_plan =
            physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_prefix_scan_tenant_status_idx")
        );
        assert_eq!(physical_plan.read.access_path, physical::ReadAccessPath::PrefixScan);
        assert_eq!(physical_plan.read.access_path_reason, "scalar-index-prefix");
    });
    }

    #[test]
    fn should_mark_range_filter_as_range_scan() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_range_scan",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_range_scan",
            "planner_range_scan_title_idx",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_range_scan WHERE title >= 'alpha' AND title < 'omega'",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_range_scan");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_range_scan_title_idx")
            );
            assert_eq!(
                physical_plan.read.access_path,
                physical::ReadAccessPath::RangeScan
            );
            assert_eq!(physical_plan.read.access_path_reason, "scalar-index-range");
        });
    }

    #[test]
    fn should_mark_order_limit_as_ordered_bounded_scan_when_index_matches() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_ordered_bounded",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_ordered_bounded",
            "planner_ordered_bounded_title_idx",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(
                "SELECT title FROM planner_ordered_bounded ORDER BY title ASC LIMIT 3",
            )
            .unwrap();
            let bound = binder::bind(parsed, &catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();

            // Act
            let indexes = catalog.list_indexes("planner_ordered_bounded");
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            let physical_plan =
                physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

            // Assert
            assert_eq!(
                physical_plan.read.selected_index.as_deref(),
                Some("planner_ordered_bounded_title_idx")
            );
            assert_eq!(
                physical_plan.read.access_path,
                physical::ReadAccessPath::OrderedBoundedScan
            );
            assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Storage);
        });
    }

    #[test]
    fn should_report_fallback_when_secondary_ordering_proof_is_missing() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_ordering_fallback",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_ordering_fallback",
            "planner_ordering_fallback_title_idx",
            vec!["title"],
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_ordering_fallback WHERE title = 'alpha' ORDER BY body ASC LIMIT 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let indexes = catalog.list_indexes("planner_ordering_fallback");
        let cardinality_stats =
            std::collections::HashMap::<String, cassie::catalog::CollectionCardinalityStats>::new();
        let physical_plan =
            physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_ordering_fallback_title_idx")
        );
        assert_eq!(physical_plan.read.access_path, physical::ReadAccessPath::CollectionScan);
        assert_eq!(
            physical_plan.read.fallback_reason.as_deref(),
            Some("index-order-proof-missing")
        );
    });
    }

    #[test]
    fn should_mark_exists_predicate_as_semi_join() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_semi_outer");
        register_test_collection(&catalog, "planner_semi_inner");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_semi_outer WHERE EXISTS (SELECT title FROM planner_semi_inner)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.join.strategy.as_deref(), Some("semi"));
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::Exists);
    });
    }

    #[test]
    fn should_mark_not_exists_predicate_as_anti_join() {
        // Arrange
        let catalog = Catalog::new();
        register_test_collection(&catalog, "planner_anti_outer");
        register_test_collection(&catalog, "planner_anti_inner");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_anti_outer WHERE NOT EXISTS (SELECT title FROM planner_anti_inner)",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert_eq!(physical_plan.join.strategy.as_deref(), Some("anti"));
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::Exists);
    });
    }

    #[test]
    fn should_build_physical_operators_for_hybrid_search() {
        // Arrange
        let catalog = Catalog::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        catalog
            .register_collection(
                "planner_hybrid_physical",
                vec![
                    ("body".to_string(), DataType::Text),
                    ("embedding".to_string(), DataType::Vector(2)),
                ],
            );
        let parsed = parser::parse_statement(
            "SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM planner_hybrid_physical ORDER BY score DESC LIMIT 1",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build(logical);

        // Assert
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::FullTextSearch)));
        assert!(physical_plan
            .operators
            .iter()
            .any(|operator| matches!(operator, Operator::VectorSearch)));
    });
    }
}

// Formerly tests/planner_read_path_depth.rs.
mod planner_read_path_depth {
    #![allow(unused_imports, dead_code)]
    use cassie::catalog::{Catalog, IndexKind, IndexMeta};
    use cassie::planner::{logical, physical};
    use cassie::sql::ast::{Expr, QueryStatement};
    use cassie::sql::{binder, parser};
    use cassie::types::{DataType, FieldSchema};
    use std::collections::BTreeMap;

    fn register_collection_fields(catalog: &Catalog, name: &str, schema: Vec<FieldSchema>) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            catalog.register_collection(
                name,
                schema
                    .into_iter()
                    .map(|field| (field.name, field.data_type))
                    .collect(),
            );
        });
    }

    fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
        let fields = fields
            .into_iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: fields.first().cloned().unwrap_or_default(),
            fields,
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    fn register_expression_index(
        catalog: &Catalog,
        collection: &str,
        name: &str,
        expression: &Expr,
    ) {
        catalog.register_index(IndexMeta {
            collection: collection.to_string(),
            name: name.to_string(),
            field: String::new(),
            fields: Vec::new(),
            expressions: vec![serde_json::to_string(expression).unwrap()],
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::new(),
        });
    }

    fn expression_from_create_index(sql: &str) -> Expr {
        let parsed = parser::parse_statement(sql).unwrap();
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };
        statement.expressions[0].clone()
    }

    fn build_plan(catalog: &Catalog, sql: &str, collection: &str) -> physical::PhysicalPlan {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let parsed = parser::parse_statement(sql).unwrap();
            let bound = binder::bind(parsed, catalog).unwrap();
            let logical = logical::plan(&bound).unwrap();
            let indexes = catalog.list_indexes(collection);
            let cardinality_stats = std::collections::HashMap::<
                String,
                cassie::catalog::CollectionCardinalityStats,
            >::new();
            physical::build_with_indexes(logical, indexes.as_slice(), &cardinality_stats)
        })
    }

    #[test]
    fn should_use_range_scan_when_mixed_order_prefix_is_equality_bound() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_mixed_order_prefix",
            vec![
                FieldSchema {
                    name: "tenant".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "created_at".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_mixed_order_prefix",
            "planner_mixed_order_prefix_idx",
            vec!["tenant", "status", "created_at"],
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT title FROM planner_mixed_order_prefix \
         WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
         ORDER BY status DESC, created_at ASC LIMIT 2",
            "planner_mixed_order_prefix",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_mixed_order_prefix_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::RangeScan
        );
        assert_eq!(physical_plan.read.access_path_reason, "scalar-index-range");
        assert_eq!(physical_plan.read.fallback_reason, None);
    }

    #[test]
    fn should_use_prefix_scan_when_mixed_order_suffix_needs_final_sort() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_mixed_order_fallback",
            vec![
                FieldSchema {
                    name: "tenant".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "created_at".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_mixed_order_fallback",
            "planner_mixed_order_fallback_idx",
            vec!["tenant", "created_at", "score"],
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT title FROM planner_mixed_order_fallback \
         WHERE tenant = 'tenant-a' \
         ORDER BY created_at DESC, score ASC LIMIT 2",
            "planner_mixed_order_fallback",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_mixed_order_fallback_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::PrefixScan
        );
        assert_eq!(physical_plan.read.access_path_reason, "scalar-index-prefix");
        assert_eq!(physical_plan.read.fallback_reason, None);
        assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
    }

    #[test]
    fn should_use_prefix_scan_when_mixed_row_id_suffix_needs_final_sort() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_mixed_order_row_id",
            vec![
                FieldSchema {
                    name: "tenant".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_mixed_order_row_id",
            "planner_mixed_order_row_id_idx",
            vec!["tenant", "score"],
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT title FROM planner_mixed_order_row_id \
         WHERE tenant = 'tenant-a' \
         ORDER BY score DESC, id ASC LIMIT 2",
            "planner_mixed_order_row_id",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_mixed_order_row_id_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::PrefixScan
        );
        assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
    }

    #[test]
    fn should_use_nonselective_prefix_scan_when_mixed_row_id_suffix_needs_final_sort() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_nonselective_mixed_order_row_id",
            vec![
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_nonselective_mixed_order_row_id",
            "planner_nonselective_mixed_order_row_id_idx",
            vec!["score"],
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT id FROM planner_nonselective_mixed_order_row_id \
         ORDER BY score DESC, id ASC LIMIT 2",
            "planner_nonselective_mixed_order_row_id",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_nonselective_mixed_order_row_id_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::PrefixScan
        );
        assert_eq!(physical_plan.read.fallback_reason, None);
        assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
    }

    #[test]
    fn should_keep_collection_scan_for_noncovered_nonselective_mixed_row_id_suffix() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_noncovered_nonselective_mixed_order",
            vec![
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        register_scalar_index(
            &catalog,
            "planner_noncovered_nonselective_mixed_order",
            "planner_noncovered_nonselective_mixed_order_idx",
            vec!["score"],
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT title FROM planner_noncovered_nonselective_mixed_order \
         ORDER BY score DESC, id ASC LIMIT 2",
            "planner_noncovered_nonselective_mixed_order",
        );

        // Assert
        assert_eq!(physical_plan.read.selected_index, None);
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::CollectionScan
        );
        assert_eq!(physical_plan.top_k.mode, physical::TopKMode::Heap);
        assert_eq!(physical_plan.read.early_stop, physical::EarlyStopMode::None);
    }

    #[test]
    fn should_lower_expression_equality_to_scalar_index_seek() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_expression_index_seek",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        let expression = expression_from_create_index(
            "CREATE INDEX planner_expression_index_seek_idx \
         ON planner_expression_index_seek USING btree (lower(title))",
        );
        register_expression_index(
            &catalog,
            "planner_expression_index_seek",
            "planner_expression_index_seek_idx",
            &expression,
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT body FROM planner_expression_index_seek WHERE lower(title) = 'alpha'",
            "planner_expression_index_seek",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_expression_index_seek_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::IndexSeek
        );
        assert_eq!(physical_plan.read.access_path_reason, "scalar-index-seek");
        assert_eq!(physical_plan.read.fallback_reason, None);
    }

    #[test]
    fn should_lower_expression_range_to_scalar_index_range_scan() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_expression_index_range",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        let expression = expression_from_create_index(
            "CREATE INDEX planner_expression_index_range_idx \
         ON planner_expression_index_range USING btree (lower(title))",
        );
        register_expression_index(
            &catalog,
            "planner_expression_index_range",
            "planner_expression_index_range_idx",
            &expression,
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT body FROM planner_expression_index_range \
         WHERE lower(title) >= 'm' AND lower(title) < 'z'",
            "planner_expression_index_range",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_expression_index_range_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::RangeScan
        );
        assert_eq!(physical_plan.read.access_path_reason, "scalar-index-range");
        assert_eq!(physical_plan.read.fallback_reason, None);
    }

    #[test]
    fn should_lower_expression_order_limit_to_ordered_bounded_scan() {
        // Arrange
        let catalog = Catalog::new();
        register_collection_fields(
            &catalog,
            "planner_expression_index_order",
            vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        );
        let expression = expression_from_create_index(
            "CREATE INDEX planner_expression_index_order_idx \
         ON planner_expression_index_order USING btree (lower(title))",
        );
        register_expression_index(
            &catalog,
            "planner_expression_index_order",
            "planner_expression_index_order_idx",
            &expression,
        );

        // Act
        let physical_plan = build_plan(
            &catalog,
            "SELECT body FROM planner_expression_index_order \
         ORDER BY lower(title) DESC LIMIT 2",
            "planner_expression_index_order",
        );

        // Assert
        assert_eq!(
            physical_plan.read.selected_index.as_deref(),
            Some("planner_expression_index_order_idx")
        );
        assert_eq!(
            physical_plan.read.access_path,
            physical::ReadAccessPath::OrderedBoundedScan
        );
        assert_eq!(
            physical_plan.read.access_path_reason,
            "scalar-index-ordered-bounded"
        );
        assert_eq!(physical_plan.read.fallback_reason, None);
    }
}

// Formerly tests/query_cancellation.rs.
mod query_cancellation {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::CassieRuntimeConfig;
    use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use std::time::Instant;
    use std::{sync::Arc, time::Duration};

    use super::support_sql as support;
    use support::*;

    #[test]
    fn should_expose_query_memory_budget_with_clean_baseline_name() {
        // Arrange
        let mut config = CassieRuntimeConfig::default();

        // Act
        config.limits.query_memory_budget_bytes = 4_096;

        // Assert
        assert_eq!(config.limits.query_memory_budget_bytes, 4_096);
    }

    #[test]
    fn should_report_cancellation_handle_state() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();

        // Act
        cancellation.cancel();

        // Assert
        assert!(cancellation.is_cancelled());
    }

    #[test]
    fn should_cancel_embedded_query_before_execution() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("embedded-query-cancelled");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        let error = cassie
            .execute_sql_with_cancellation(&session, "SELECT 1", vec![], &cancellation)
            .expect_err("cancelled query should stop before execution");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_query_memory_above_remaining_budget() {
        // Arrange
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_memory_budget_bytes = 10;
        let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
        let reservation = controls.reserve_query_memory(8).expect("first reservation");

        // Act
        let error = controls
            .reserve_query_memory(3)
            .expect_err("combined reservations should exceed the budget");

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(controls.peak_query_memory_bytes(), 8);
        drop(reservation);
    }

    #[test]
    fn should_allow_memory_reservation_after_a_prior_reservation_is_dropped() {
        // Arrange
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_memory_budget_bytes = 10;
        let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
        let reservation = controls.reserve_query_memory(10).expect("full reservation");
        drop(reservation);

        // Act
        let replacement = controls
            .reserve_query_memory(10)
            .expect("released bytes should be reusable");

        // Assert
        assert_eq!(controls.peak_query_memory_bytes(), 10);
        drop(replacement);
    }

    #[test]
    fn should_cancel_embedded_query_during_recursive_execution() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("embedded-query-active-cancel");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Arc::new(
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie"),
        );
        cassie.startup().expect("startup");
        let cancellation = QueryCancellationHandle::new();
        let query_cancellation = cancellation.clone();
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            let session = query_cassie.create_session("tester", None);
            query_cassie.execute_sql_with_cancellation(
            &session,
            "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq",
            vec![],
            &query_cancellation,
        )
        });
        std::thread::sleep(Duration::from_millis(25));

        // Act
        cancellation.cancel();
        let error = query
            .join()
            .expect("query thread")
            .expect_err("active query should be cancelled");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_embedded_query_during_ordered_execution() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("embedded-ordered-query-cancel");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Arc::new(
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie"),
        );
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE cancellation_sort_rows (value TEXT)",
                vec![],
            )
            .expect("create table");
        let rows = (0..20_000)
            .map(|index| {
                (
                    Some(format!("row-{index:05}")),
                    serde_json::json!({"value": format!("value-{:05}", 20_000 - index)}),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents("cancellation_sort_rows", rows)
            .expect("seed rows");
        let cancellation = QueryCancellationHandle::new();
        let query_cancellation = cancellation.clone();
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            let session = query_cassie.create_session("tester", None);
            query_cassie.execute_sql_with_cancellation(
                &session,
                "SELECT value FROM cancellation_sort_rows ORDER BY value",
                vec![],
                &query_cancellation,
            )
        });
        std::thread::sleep(Duration::from_millis(10));

        // Act
        cancellation.cancel();
        let error = query
            .join()
            .expect("query thread")
            .expect_err("ordered query should be cancelled");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));

        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/query_promotion_evidence.rs.
mod query_promotion_evidence {
    use super::support_query_evidence as query_evidence;

    #[test]
    fn should_compare_seeded_indexed_pages_with_overlay() {
        // Arrange
        let fixture = query_evidence::SeededQueryFixture::from_seed(0x00C4_551E, 37);
        let different_seed = query_evidence::SeededQueryFixture::from_seed(0xA11C_E002, 37);

        // Act
        let pages = fixture.compare_indexed_pages_with_overlay();
        let different_seed_pages = different_seed.compare_indexed_pages_with_overlay();

        // Assert
        assert_eq!(pages.indexed, pages.row_baseline);
        assert_eq!(pages.indexed.len(), 14);
        assert!(pages.indexed.iter().all(|page| page.len() <= 3));
        assert!(pages.indexed[0].iter().any(|row| {
            row == &vec![
                cassie::types::Value::String("overlay".to_owned()),
                cassie::types::Value::Int64(100),
            ]
        }));
        assert_eq!(pages.indexed.concat(), pages.indexed_full);
        assert_eq!(pages.row_baseline.concat(), pages.row_baseline_full);
        assert_eq!(pages.indexed_empty, pages.row_baseline_empty);
        assert!(pages.indexed_empty.is_empty());
        assert_eq!(pages.indexed_full, pages.indexed_rewritten);
        assert_eq!(pages.row_baseline_full, pages.row_baseline_rewritten);
        assert_eq!(pages.indexed_rewritten, pages.row_baseline_rewritten);
        assert_ne!(pages.indexed_full, different_seed_pages.indexed_full);
        assert_eq!(
            different_seed_pages.indexed,
            different_seed_pages.row_baseline
        );
    }
}

// Formerly tests/statement_routes.rs.
mod statement_routes {
    use cassie::sql::ast::{
        CatalogStatement, CatalogStatementRef, ProjectionStatement, ProjectionStatementRef,
        RetentionStatement, RetentionStatementRef, RuntimeStatement, RuntimeStatementRef,
        StatementFamily, StatementRoute, StatementRouteRef,
    };
    use cassie::sql::parser::parse_statement;

    fn parsed_route(sql: &str) -> (StatementFamily, cassie::sql::ast::QueryStatement) {
        let parsed = parse_statement(sql).expect("statement should parse");
        (parsed.statement.family(), parsed.statement)
    }

    #[test]
    fn should_preserve_owned_statement_routes_for_every_family() {
        // Arrange
        let runtime = parse_statement("SELECT 1")
            .expect("runtime statement")
            .statement;
        let catalog = parse_statement("CREATE TABLE route_docs (title TEXT)")
            .expect("catalog statement")
            .statement;
        let projection = parse_statement(
        "CREATE ROLLUP route_rollup ON route_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
    )
    .expect("projection statement")
    .statement;
        let retention = parse_statement(
        "CREATE RETENTION POLICY route_retention ON route_events USING event_at RETAIN FOR '7 days'",
    )
    .expect("retention statement")
    .statement;

        // Act
        let routes = [
            runtime.into_route(),
            catalog.into_route(),
            projection.into_route(),
            retention.into_route(),
        ];

        // Assert
        assert!(matches!(
            routes[0],
            StatementRoute::Runtime(RuntimeStatement::Select(_))
        ));
        assert!(matches!(
            routes[1],
            StatementRoute::Catalog(CatalogStatement::CreateTable(_))
        ));
        assert!(matches!(
            routes[2],
            StatementRoute::Projection(ProjectionStatement::CreateRollup(_))
        ));
        assert!(matches!(
            routes[3],
            StatementRoute::Retention(RetentionStatement::CreateRetentionPolicy(_))
        ));
    }

    #[test]
    fn should_route_select_statements_through_runtime_family() {
        // Arrange

        // Act
        let (family, statement) = parsed_route("SELECT title FROM route_docs");

        // Assert
        assert_eq!(family, StatementFamily::Runtime);
        assert!(matches!(
            statement.route(),
            StatementRouteRef::Runtime(RuntimeStatementRef::Select(_))
        ));
    }

    #[test]
    fn should_route_create_table_statements_through_catalog_family() {
        // Arrange

        // Act
        let (family, statement) = parsed_route("CREATE TABLE route_docs (title TEXT)");

        // Assert
        assert_eq!(family, StatementFamily::Catalog);
        assert!(matches!(
            statement.route(),
            StatementRouteRef::Catalog(CatalogStatementRef::CreateTable(_))
        ));
    }

    #[test]
    fn should_route_projection_statements_through_projection_family() {
        // Arrange

        // Act
        let (family, statement) = parsed_route(
        "CREATE ROLLUP route_rollup ON route_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
    );

        // Assert
        assert_eq!(family, StatementFamily::Projection);
        assert!(matches!(
            statement.route(),
            StatementRouteRef::Projection(ProjectionStatementRef::CreateRollup(_))
        ));
    }

    #[test]
    fn should_route_retention_statements_through_retention_family() {
        // Arrange

        // Act
        let (family, statement) = parsed_route(
        "CREATE RETENTION POLICY route_retention ON route_events USING event_at RETAIN FOR '7 days'",
    );

        // Assert
        assert_eq!(family, StatementFamily::Retention);
        assert!(matches!(
            statement.route(),
            StatementRouteRef::Retention(RetentionStatementRef::CreateRetentionPolicy(_))
        ));
    }

    #[test]
    fn should_route_transaction_statements_through_runtime_family() {
        // Arrange

        // Act
        let (family, statement) = parsed_route("BEGIN");

        // Assert
        assert_eq!(family, StatementFamily::Runtime);
        assert!(matches!(
            statement.route(),
            StatementRouteRef::Runtime(RuntimeStatementRef::Transaction(_))
        ));
    }
}
