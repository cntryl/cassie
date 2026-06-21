#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::sql::ast::QueryStatement;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_projection_diff_command() {
    // Arrange
    let diff_sql =
        "DIFF PROJECTION left_docs VERSION v1 WITH right_docs VERSION v2 LIMIT 10 AFTER row-1";

    // Act
    let diff = cassie::sql::parse_statement(diff_sql).unwrap();

    // Assert
    let QueryStatement::DiffProjection(diff) = diff.statement else {
        panic!("expected DIFF PROJECTION");
    };
    assert_eq!(diff.left.name, "left_docs");
    assert_eq!(diff.left.version_id.as_deref(), Some("v1"));
    assert_eq!(diff.right.name, "right_docs");
    assert_eq!(diff.right.version_id.as_deref(), Some("v2"));
    assert_eq!(diff.limit, Some(10));
    assert_eq!(diff.after.as_deref(), Some("row-1"));
}

#[test]
fn should_parse_projection_compare_command() {
    // Arrange
    let compare_sql = "COMPARE PROJECTION left_docs WITH MANIFEST '{\"root_digest\":\"abc\"}'";

    // Act
    let compare = cassie::sql::parse_statement(compare_sql).unwrap();

    // Assert
    let QueryStatement::CompareProjection(compare) = compare.statement else {
        panic!("expected COMPARE PROJECTION");
    };
    assert_eq!(compare.target.name, "left_docs");
    assert!(compare.manifest.contains("root_digest"));
}

#[test]
fn should_diff_projection_hashes_deterministically() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_diff_hashes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE diff_left (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(&session, "CREATE TABLE diff_right (title TEXT)", vec![])
            .unwrap();
        cassie
            .midge
            .put_document(
                "diff_left",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "diff_right",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "bravo"}),
            )
            .unwrap();

        // Act
        let diff = cassie
            .execute_sql(
                &session,
                "DIFF PROJECTION diff_left WITH diff_right LIMIT 5",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(diff.columns[0].name, "row_id");
        assert_eq!(diff.rows[0][0], Value::String("doc-1".to_string()));
        assert_eq!(diff.rows[0][1], Value::String("changed".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_compare_projection_manifest_root_digest() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_compare_manifest");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE compare_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .midge
            .put_document(
                "compare_docs",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let root = cassie
            .midge
            .root_hash("compare_docs")
            .unwrap()
            .expect("root hash");
        let sql = format!(
            "COMPARE PROJECTION compare_docs WITH MANIFEST '{{\"root_digest\":\"{}\",\"algorithm\":\"{}\",\"digest_length\":{},\"canonical_encoder_version\":{},\"row_hash_version\":{},\"range_hash_version\":{},\"root_hash_version\":{}}}'",
            root.digest,
            root.algorithm,
            root.digest_length,
            root.canonical_encoder_version,
            root.row_hash_version,
            root.range_hash_version,
            root.root_hash_version
        );

        // Act
        let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();

        // Assert
        assert_eq!(compared.rows[0][1], Value::String("equal".to_string()));
        assert_eq!(compared.rows[0][4], Value::Int64(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_projection_diff_resume_cursor_for_bounded_output() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_diff_resume_cursor");
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
                "CREATE TABLE diff_cursor_left (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE diff_cursor_right (title TEXT)",
                vec![],
            )
            .unwrap();
        for row_id in ["doc-1", "doc-2", "doc-3"] {
            cassie
                .midge
                .put_document(
                    "diff_cursor_left",
                    Some(row_id.to_string()),
                    serde_json::json!({"title": "left"}),
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "diff_cursor_right",
                    Some(row_id.to_string()),
                    serde_json::json!({"title": "right"}),
                )
                .unwrap();
        }

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "DIFF PROJECTION diff_cursor_left WITH diff_cursor_right LIMIT 1",
                vec![],
            )
            .unwrap();
        let cursor = match &first.rows[0][5] {
            Value::String(value) => value.clone(),
            other => panic!("expected resume cursor, got {other:?}"),
        };
        let second = cassie
            .execute_sql(
                &session,
                &format!(
                    "DIFF PROJECTION diff_cursor_left WITH diff_cursor_right LIMIT 5 AFTER {}",
                    cursor
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.rows[0][0], Value::String("doc-1".to_string()));
        assert_eq!(first.rows[0][6], Value::Bool(false));
        assert_eq!(second.rows[0][0], Value::String("doc-2".to_string()));
        assert_eq!(second.rows[0][6], Value::Bool(true));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_projection_manifest_missing_hash_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_compare_missing_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE compare_missing_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .midge
            .put_document(
                "compare_missing_docs",
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let root = cassie
            .midge
            .root_hash("compare_missing_docs")
            .unwrap()
            .expect("root hash");
        let sql = format!(
            "COMPARE PROJECTION compare_missing_docs WITH MANIFEST '{{\"root_digest\":\"{}\"}}'",
            root.digest
        );

        // Act
        let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                "SELECT state, compatibility_status FROM pg_catalog.pg_projection_comparison_reports",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(compared.rows[0][1], Value::String("unverifiable".to_string()));
        assert_eq!(
            reports.rows[0],
            vec![
                Value::String("unverifiable".to_string()),
                Value::String("incompatible-missing-algorithm".to_string()),
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_projection_comparison_report_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_comparison_report_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let report_id = {
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE comparison_report_docs (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .midge
                .put_document(
                    "comparison_report_docs",
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )
                .unwrap();
            let root = cassie
                .midge
                .root_hash("comparison_report_docs")
                .unwrap()
                .expect("root hash");
            let sql = format!(
                "COMPARE PROJECTION comparison_report_docs WITH MANIFEST '{{\"root_digest\":\"{}\",\"algorithm\":\"{}\",\"digest_length\":{},\"canonical_encoder_version\":{},\"row_hash_version\":{},\"range_hash_version\":{},\"root_hash_version\":{}}}'",
                root.digest,
                root.algorithm,
                root.digest_length,
                root.canonical_encoder_version,
                root.row_hash_version,
                root.range_hash_version,
                root.root_hash_version
            );

            // Act
            let compared = cassie.execute_sql(&session, &sql, vec![]).unwrap();
            let report_id = match &compared.rows[0][5] {
                Value::String(value) => value.clone(),
                other => panic!("expected report id, got {other:?}"),
            };
            cassie.shutdown();
            report_id
        };

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let reports = restarted
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, compatibility_status, mismatch_count FROM pg_catalog.pg_projection_comparison_reports WHERE report_id = '{}'",
                    report_id
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            reports.rows,
            vec![vec![
                Value::String("equal".to_string()),
                Value::String("compatible".to_string()),
                Value::Int64(0),
            ]]
        );

        restarted.shutdown();
        let _ = std::fs::remove_dir_all(path);
    });
}
