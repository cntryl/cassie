use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::QueryCancellationHandle;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use super::support_sql as support;
use support::data_dir;

fn with_copy_table(test_name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    let path = data_dir(test_name);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_boundary_rows (id INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create table");
    test(&cassie, &session);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

fn copy_statement() -> CopyStatement {
    CopyStatement {
        table: "copy_boundary_rows".to_string(),
        columns: vec!["id".to_string(), "title".to_string()],
        format: CopyFormat::Csv,
        header: false,
    }
}

fn selected_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT title FROM copy_boundary_rows ORDER BY title",
            vec![],
        )
        .expect("select rows")
        .rows()
        .to_vec()
}

#[test]
fn should_rollback_copy_from_stdin_given_a_malformed_csv_row() {
    // Arrange
    with_copy_table("copy-malformed-row", |cassie, session| {
        let statement = copy_statement();

        // Act
        let result =
            cassie.copy_from_csv_stdin(session, &statement, b"1,alpha\n2,\"unterminated\n");

        // Assert
        assert!(result.is_err());
        assert_eq!(
            selected_rows(cassie, session),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );
    });
}

#[test]
fn should_preserve_prior_rows_given_a_later_copy_row_failure() {
    // Arrange
    with_copy_table("copy-later-row-failure", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "INSERT INTO copy_boundary_rows (id, title) VALUES (1, 'existing')",
                vec![],
            )
            .expect("seed existing row");
        let statement = copy_statement();

        // Act
        let result = cassie.copy_from_csv_stdin(
            session,
            &statement,
            b"2,staged-before-error\nnot-an-integer,invalid\n",
        );

        // Assert
        assert!(result.is_err());
        assert_eq!(
            selected_rows(cassie, session),
            vec![vec![Value::String("existing".into())]]
        );
    });
}

#[test]
fn should_reject_copy_inside_a_failed_transaction() {
    // Arrange
    with_copy_table("copy-failed-transaction", |cassie, session| {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(session, "SELECT 1 / 0", vec![])
            .expect_err("fail transaction");

        // Act
        let result = cassie.copy_from_csv_stdin(session, &copy_statement(), b"1,blocked\n");

        // Assert
        assert!(result.is_err());
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback");
    });
}

#[test]
fn should_require_rollback_after_copy_failure() {
    // Arrange
    with_copy_table("copy-requires-rollback", |cassie, session| {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .copy_from_csv_stdin(session, &copy_statement(), b"invalid,blocked\n")
            .expect_err("copy failure");

        // Act
        let blocked = cassie.execute_sql(session, "SELECT 1", vec![]);
        let rollback = cassie.execute_sql(session, "ROLLBACK", vec![]);
        let recovered = cassie.execute_sql(session, "SELECT 1", vec![]);

        // Assert
        assert!(blocked.is_err());
        assert!(rollback.is_ok());
        assert!(recovered.is_ok());
    });
}

#[test]
fn should_cancel_copy_before_partial_commit() {
    // Arrange
    with_copy_table("copy-cancel-before-commit", |cassie, session| {
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        let result = cassie.copy_from_csv_stdin_with_cancellation(
            session,
            &copy_statement(),
            b"1,first\n2,second\n",
            &cancellation,
        );

        // Assert
        assert!(result.is_err());
        assert_eq!(
            selected_rows(cassie, session),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );
    });
}

#[test]
fn should_rollback_transactional_copy_without_publishing_rows() {
    // Arrange
    with_copy_table("copy-transaction-rollback", |cassie, session| {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin COPY transaction");
        cassie
            .copy_from_csv_stdin(session, &copy_statement(), b"1,staged\n")
            .expect("stage COPY row");

        // Act
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback COPY transaction");

        // Assert
        assert_eq!(
            selected_rows(cassie, session),
            [] as [std::vec::Vec<cassie::types::Value>; 0]
        );
    });
}

#[test]
fn should_restore_warm_publication_state_after_transactional_copy_rollback() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("copy-rollback-publication-state", None);
    for sql in [
        "CREATE TABLE copy_rollback_state \
         (id INT PRIMARY KEY, tenant TEXT, ts TIMESTAMP, n INT, tag TEXT, title TEXT)",
        "INSERT INTO copy_rollback_state \
         VALUES (1, 'a', '2024-01-01 00:05:00', 1, 'kept', 'before')",
        "CREATE INDEX copy_rollback_state_idx ON copy_rollback_state \
         USING column (tag, title) WITH (segment_size = 1)",
        "CREATE MATERIALIZED PROJECTION copy_rollback_projection \
         AS SELECT id, title FROM copy_rollback_state",
        "CREATE ROLLUP copy_rollback_rollup ON copy_rollback_state \
         USING time_bucket('1 day', ts) GROUP BY tenant \
         AGGREGATES COUNT(*) AS total",
        "REFRESH ROLLUP copy_rollback_rollup",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("set up COPY rollback publication state");
    }
    let collection = support::canonical_test_collection(&cassie, "copy_rollback_state");
    let projection_name = cassie
        .catalog
        .get_materialized_projection("copy_rollback_projection")
        .expect("projection metadata")
        .collection;
    let query = "SELECT title FROM copy_rollback_state WHERE tag = 'kept' ORDER BY id";
    let select_rows = || {
        cassie
            .execute_sql(&session, query, Vec::new())
            .expect("warm transactional COPY query")
            .rows()
            .to_vec()
    };
    let cache_hits = || {
        cassie.metrics()["execution_result_cache"]["hits"]
            .as_u64()
            .expect("cache hit metric")
    };
    let column_scans = || {
        cassie.metrics()["column_batches"]["scans"]
            .as_u64()
            .expect("column batch scan metric")
    };
    let projection_state = || {
        cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_materialized_projections \
                     WHERE projection_name = '{projection_name}'"
                ),
                Vec::new(),
            )
            .expect("read projection state")
            .rows()
            .to_vec()
    };
    let rollup_state = || {
        cassie
            .execute_sql(
                &session,
                "SELECT state FROM pg_catalog.pg_rollups \
                 WHERE rollup_name = 'copy_rollback_rollup'",
                Vec::new(),
            )
            .expect("read rollup state")
            .rows()
            .to_vec()
    };
    let rows_before = select_rows();
    let warmed = select_rows();
    let hits_before_rollback = cache_hits();
    let scans_before = column_scans();
    let generation_before = cassie
        .midge
        .collection_generation(&collection)
        .expect("source generation");
    let cardinality_before = cassie
        .catalog
        .get_cardinality_stats(&collection)
        .expect("warmed cardinality state");
    let projection_state_before = projection_state();
    let rollup_state_before = rollup_state();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin COPY rollback transaction");
    let statement = CopyStatement {
        table: "copy_rollback_state".to_string(),
        columns: vec![
            "id".to_string(),
            "tenant".to_string(),
            "ts".to_string(),
            "n".to_string(),
            "tag".to_string(),
            "title".to_string(),
        ],
        format: CopyFormat::Csv,
        header: false,
    };
    cassie
        .copy_from_csv_stdin(
            &session,
            &statement,
            b"2,a,2024-01-01 00:20:00,2,kept,after\n",
        )
        .expect("stage COPY row");

    // Act
    cassie
        .execute_sql(&session, "ROLLBACK", Vec::new())
        .expect("rollback transactional COPY");
    let cached_after_rollback = select_rows();
    let hits_after_rollback = cache_hits();
    let column_rows_after_rollback = cassie
        .execute_sql(
            &session,
            "SELECT title FROM copy_rollback_state \
             WHERE tag = 'kept' AND id > 0 ORDER BY id",
            Vec::new(),
        )
        .expect("read column batches after rollback")
        .rows()
        .to_vec();
    let projection_rows = cassie
        .execute_sql(
            &session,
            "SELECT title FROM copy_rollback_projection ORDER BY id",
            Vec::new(),
        )
        .expect("read projection after rollback")
        .rows()
        .to_vec();
    let rollup_rows = cassie
        .execute_sql(
            &session,
            "SELECT time_bucket('1 day', ts) AS bucket, tenant, COUNT(*) AS total \
             FROM copy_rollback_state GROUP BY time_bucket('1 day', ts), tenant",
            Vec::new(),
        )
        .expect("read rollup-backed aggregate after rollback")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows_before, vec![vec![Value::String("before".into())]]);
    assert_eq!(warmed, rows_before);
    assert_eq!(cached_after_rollback, rows_before);
    assert_eq!(hits_after_rollback, hits_before_rollback + 1);
    assert_eq!(column_rows_after_rollback, rows_before);
    assert!(
        column_scans() > scans_before,
        "the post-rollback read must exercise refreshed column-batch content"
    );
    assert_eq!(
        cassie
            .midge
            .collection_generation(&collection)
            .expect("post-rollback source generation"),
        generation_before
    );
    assert_eq!(
        cassie
            .catalog
            .get_cardinality_stats(&collection)
            .expect("post-rollback cardinality state"),
        cardinality_before
    );
    assert_eq!(projection_state(), projection_state_before);
    assert_eq!(projection_rows, vec![vec![Value::String("before".into())]]);
    assert_eq!(rollup_state(), rollup_state_before);
    assert_eq!(rollup_rows[0][2], Value::Int64(1));
}

#[test]
fn should_publish_copy_through_warm_cache_and_column_batch_reads_after_commit() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE copy_cache_parity (id INT PRIMARY KEY, tag TEXT, title TEXT)",
        "INSERT INTO copy_cache_parity VALUES (1, 'kept', 'before')",
        "CREATE INDEX copy_cache_parity_idx ON copy_cache_parity USING column (tag, title) WITH (segment_size = 1)",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("setup COPY cache fixture");
    }
    let query = "SELECT title FROM copy_cache_parity WHERE tag = 'kept'";
    let select_rows = || {
        cassie
            .execute_sql(&session, query, Vec::new())
            .expect("read COPY cache fixture")
            .rows()
            .to_vec()
    };
    let cache_hits = || {
        cassie.metrics()["execution_result_cache"]["hits"]
            .as_u64()
            .expect("cache hit metric")
    };
    let first = select_rows();
    let column_scans_before = cassie.metrics()["column_batches"]["scans"]
        .as_u64()
        .expect("column batch scan metric");
    let hits_before_warm = cache_hits();

    // Act
    let warmed = select_rows();
    let hits_after_warm = cache_hits();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin COPY transaction");
    let statement = CopyStatement {
        table: "copy_cache_parity".to_string(),
        columns: vec!["id".to_string(), "tag".to_string(), "title".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };
    cassie
        .copy_from_csv_stdin(&session, &statement, b"2,kept,after\n")
        .expect("stage COPY rows");
    cassie
        .execute_sql(&session, "COMMIT", Vec::new())
        .expect("commit COPY transaction");
    let rows_after_copy = select_rows();
    let hits_after_copy = cache_hits();
    let column_scans_after = cassie.metrics()["column_batches"]["scans"]
        .as_u64()
        .expect("column batch scan metric");
    let count_after_copy = cassie
        .execute_sql(
            &session,
            "SELECT COUNT(*) FROM copy_cache_parity",
            Vec::new(),
        )
        .expect("count committed COPY rows")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(first, vec![vec![Value::String("before".into())]]);
    assert_eq!(warmed, first);
    assert_eq!(
        hits_after_warm,
        hits_before_warm + 1,
        "the repeated pre-COPY query must prove an actual cache hit"
    );
    assert_eq!(rows_after_copy.len(), 2);
    assert!(rows_after_copy.contains(&vec![Value::String("before".into())]));
    assert!(rows_after_copy.contains(&vec![Value::String("after".into())]));
    assert_eq!(
        hits_after_copy, hits_after_warm,
        "the query after COPY must not return a stale cached result"
    );
    assert!(
        column_scans_before > 0,
        "the baseline read must exercise the column-batch path"
    );
    assert!(
        column_scans_after > column_scans_before,
        "the post-COPY read must refresh through the column-batch path"
    );
    // Explicit transactions share INSERT's commit finishing; COPY's extra
    // cardinality refresh belongs only to the direct autocommit COPY path.
    // This count matches PostgreSQL's result for the same two-row input.
    assert_eq!(count_after_copy, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_mark_a_source_materialized_projection_stale_after_transactional_copy() {
    // Arrange
    with_copy_table("copy-transaction-projection-stale", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE MATERIALIZED PROJECTION copy_boundary_projection \
                 AS SELECT id, title FROM copy_boundary_rows WHERE id > 0",
                vec![],
            )
            .expect("create materialized source projection");
        let projection_name = cassie
            .catalog
            .get_materialized_projection("copy_boundary_projection")
            .expect("projection metadata")
            .collection;
        let state = || {
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "SELECT state FROM pg_catalog.pg_materialized_projections \
                         WHERE projection_name = '{projection_name}'"
                    ),
                    vec![],
                )
                .expect("read projection state through the public catalog")
                .rows()
                .to_vec()
        };
        assert_eq!(state(), vec![vec![Value::String("ready".into())]]);

        // Act
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin COPY transaction");
        cassie
            .copy_from_csv_stdin(session, &copy_statement(), b"1,after\n")
            .expect("stage COPY row");
        cassie
            .execute_sql(session, "COMMIT", vec![])
            .expect("commit COPY transaction");

        // Assert
        assert_eq!(state(), vec![vec![Value::String("stale".into())]]);
        assert_eq!(
            cassie
                .execute_sql(session, "SELECT COUNT(*) FROM copy_boundary_rows", vec![])
                .expect("count committed source rows")
                .rows()
                .to_vec(),
            vec![vec![Value::Int64(1)]]
        );
    });
}
