use cassie::app::Cassie;
use cassie::midge::adapter::{RowHashRecord, StorageFamily};
use cassie::sql::ast::{ProjectionRepairScope, QueryStatement};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn corrupt_first_row_hash(cassie: &Cassie, collection: &str) {
    let collection = canonical_test_collection(cassie, collection);
    let mut row_hash = cassie.midge.list_row_hashes(&collection).unwrap()[0].clone();
    let key = row_hash_storage_key(cassie, &collection, &row_hash.row_id);
    row_hash.state = cassie::midge::adapter::StoredHashState::Stale;
    let mut tx = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadWrite)
        .unwrap();
    tx.put(key, serde_json::to_vec(&row_hash).unwrap(), None)
        .unwrap();
    tx.commit(cntryl_midge::WriteOptions::sync()).unwrap();
}

fn row_hash_storage_key(cassie: &Cassie, collection: &str, row_id: &str) -> Vec<u8> {
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap()
        .into_iter()
        .find_map(|(key, value)| {
            let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
            (record.collection == collection && record.row_id == row_id).then_some(key)
        })
        .expect("row hash key should exist")
}

#[test]
fn should_parse_projection_repair_commands() {
    // Arrange
    let plan_sql = "PLAN REPAIR PROJECTION repair_docs VERSION v2 SCOPE range";
    let repair_sql = "REPAIR PROJECTION repair_docs SCOPE full-rebuild";

    // Act
    let plan = cassie::sql::parse_statement(plan_sql).unwrap();
    let repair = cassie::sql::parse_statement(repair_sql).unwrap();

    // Assert
    let QueryStatement::PlanRepairProjection(plan) = plan.statement else {
        panic!("expected PLAN REPAIR PROJECTION");
    };
    assert_eq!(plan.target.name, "repair_docs");
    assert_eq!(plan.target.version_id.as_deref(), Some("v2"));
    assert_eq!(plan.scope, ProjectionRepairScope::Range);

    let QueryStatement::RepairProjection(repair) = repair.statement else {
        panic!("expected REPAIR PROJECTION");
    };
    assert_eq!(repair.target.name, "repair_docs");
    assert_eq!(repair.scope, ProjectionRepairScope::FullRebuild);
}

#[test]
fn should_plan_dry_run_repair_from_integrity_findings() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_repair_plan");
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
                "CREATE TABLE repair_plan_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_plan_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "repair_plan_docs");
        corrupt_first_row_hash(&cassie, "repair_plan_docs");
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_plan_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Act
        let scopes = [
            ("row", "rebuild_projection_hashes", true),
            ("range", "rebuild_projection_hashes", true),
            ("index", "rebuild_index_entries", false),
            ("projection-version", "refresh_projection_version", false),
            ("full-rebuild", "refresh_materialized_projection", false),
        ];
        let plans = scopes
            .into_iter()
            .map(|(scope, action, executable)| {
                let plan = cassie
                    .execute_sql(
                        &session,
                        &format!("PLAN REPAIR PROJECTION repair_plan_docs SCOPE {scope}"),
                        vec![],
                    )
                    .unwrap();
                (scope, action, executable, plan)
            })
            .collect::<Vec<_>>();

        // Assert
        for (scope, action, executable, plan) in plans {
            assert_eq!(plan.command, "PLAN REPAIR PROJECTION");
            assert_eq!(plan.rows[0][0], Value::String("planned".to_string()));
            assert_eq!(plan.rows[0][2], Value::String(scope.replace('-', "_")));
            assert_eq!(plan.rows[0][4], Value::String(action.to_string()));
            assert_eq!(plan.rows[0][5], Value::Bool(executable));
            assert_eq!(
                plan.rows[0][6],
                Value::String(format!("VERIFY PROJECTION {projection} MODE full"))
            );
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_verified_local_hash_repair_with_audit() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_repair_execute");
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
                "CREATE TABLE repair_execute_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_execute_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "repair_execute_docs");
        corrupt_first_row_hash(&cassie, "repair_execute_docs");
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_execute_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Act
        let repair = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_execute_docs SCOPE row",
                vec![],
            )
            .unwrap();
        let audit = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state, scope, action, post_verification_state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(repair.command, "REPAIR PROJECTION");
        assert_eq!(repair.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(
            repair.rows[0][7],
            Value::String("verified".to_string())
        );
        assert_eq!(audit.rows[0][0], Value::String("completed".to_string()));
        assert_eq!(audit.rows[0][1], Value::String("row".to_string()));
        assert_eq!(
            audit.rows[0][2],
            Value::String("rebuild_projection_hashes".to_string())
        );
        assert_eq!(audit.rows[0][3], Value::String("verified".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_unsafe_repair_scope_deterministically() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_repair_unsafe");
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
                "CREATE TABLE repair_unsafe_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_unsafe_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_unsafe_docs MODE full",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "REPAIR PROJECTION repair_unsafe_docs SCOPE index",
                vec![],
            )
            .expect_err("index repair should require index-specific findings");

        // Assert
        assert!(error
            .to_string()
            .contains("repair scope 'index' is not executable"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_run_repair_from_query_path() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_repair_query_path");
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
                "CREATE TABLE repair_query_path_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO repair_query_path_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        let projection = canonical_test_collection(&cassie, "repair_query_path_docs");
        corrupt_first_row_hash(&cassie, "repair_query_path_docs");
        cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION repair_query_path_docs MODE hashes_only",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM repair_query_path_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let reports = cassie
            .execute_sql(
                &session,
                &format!(
                    "SELECT state FROM pg_catalog.pg_projection_repair_reports WHERE projection_name = '{projection}'"
                ),
                vec![],
            )
            .unwrap();

        // Assert
        assert!(reports.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}
