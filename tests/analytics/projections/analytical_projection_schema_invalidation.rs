#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::types::Value;

use super::support_sql as support;
use support::*;

#[test]
fn should_fall_back_after_renaming_columns_used_by_projection() {
    // Arrange
    use_local_storage();
    let path = data_dir("analytical_projection_column_rename_invalidation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE schema_rename_source (a INT, b INT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO schema_rename_source VALUES (1, 2)",
                    vec![],
                )
                .expect("insert source row");
            cassie
                .execute_sql(
                    &session,
                    "CREATE MATERIALIZED PROJECTION schema_rename_projection WITH (analytical = true) AS SELECT a, b FROM schema_rename_source",
                    vec![],
                )
                .expect("create projection");

            // Act
            for (from, to) in [("a", "tmp"), ("b", "a"), ("tmp", "b")] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("ALTER TABLE schema_rename_source RENAME COLUMN {from} TO {to}"),
                        vec![],
                    )
                    .expect("rename column");
            }
            let before = cassie.metrics();
            let result = cassie
                .execute_sql(&session, "SELECT a, b FROM schema_rename_source", vec![])
                .expect("query renamed source");
            let after = cassie.metrics();

            // Assert
            assert_eq!(result.rows(), vec![vec![Value::Int64(2), Value::Int64(1)]]);
            assert!(
                after["projections"]["mixed_execution_fallbacks"]
                    .as_u64()
                    .expect("fallback metric")
                    > before["projections"]["mixed_execution_fallbacks"]
                        .as_u64()
                        .expect("fallback metric")
            );
            assert_eq!(
                after["projections"]["last_fallback_reason"].as_str(),
                Some("stale-or-unverified")
            );
            let _ = std::fs::remove_dir_all(&path);
        });
}

#[test]
fn should_fall_back_after_projected_column_name_is_reused() {
    // Arrange
    use_local_storage();
    let path = data_dir("analytical_projection_column_drop_invalidation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE schema_drop_source (tenant TEXT, amount INT, legacy INT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO schema_drop_source VALUES ('acme', 1, 99)",
                    vec![],
                )
                .expect("insert source row");
            cassie
                .execute_sql(
                    &session,
                    "CREATE MATERIALIZED PROJECTION schema_drop_projection WITH (analytical = true) AS SELECT tenant, amount FROM schema_drop_source",
                    vec![],
                )
                .expect("create projection");

            // Act
            cassie
                .execute_sql(&session, "ALTER TABLE schema_drop_source DROP COLUMN amount", vec![])
                .expect("drop projected column");
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE schema_drop_source RENAME COLUMN legacy TO amount",
                    vec![],
                )
                .expect("reuse dropped column name");
            let before = cassie.metrics();
            let result = cassie
                .execute_sql(
                    &session,
                    "SELECT tenant, amount FROM schema_drop_source",
                    vec![],
                )
                .expect("query updated source");
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                result.rows(),
                vec![vec![Value::String("acme".to_string()), Value::Int64(99)]]
            );
            assert!(
                after["projections"]["mixed_execution_fallbacks"]
                    .as_u64()
                    .expect("fallback metric")
                    > before["projections"]["mixed_execution_fallbacks"]
                        .as_u64()
                        .expect("fallback metric")
            );
            assert_eq!(
                after["projections"]["last_fallback_reason"].as_str(),
                Some("stale-or-unverified")
            );
            let _ = std::fs::remove_dir_all(&path);
        });
}
