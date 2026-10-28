//! REINDEX repairs stored scalar entries without rematerializing query results.
use crate::support_scalar_index_rebuild::{entries, remove_entries};
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_rebuild_unqualified_table_scalar_entries() {
    // Arrange
    let fixture = sql_fixture(
        "reindex-bare-table",
        &[
            "CREATE TABLE repair_rows(value INT)",
            "INSERT INTO repair_rows VALUES(7)",
            "CREATE INDEX repair_idx ON repair_rows(value)",
        ],
    );
    let before = remove_entries(&fixture.cassie, "repair_rows");
    // Act
    fixture.execute("REINDEX TABLE repair_rows").unwrap();
    // Assert
    assert_eq!(entries(&fixture.cassie, "repair_rows"), before);
}

#[test]
fn should_rebuild_active_materialized_scalar_entries() {
    // Arrange
    let fixture = sql_fixture(
        "reindex-materialized",
        &[
            "CREATE TABLE repair_source(value INT)",
            "INSERT INTO repair_source VALUES(7)",
            "CREATE MATERIALIZED VIEW repair_view AS SELECT value FROM repair_source",
            "CREATE UNIQUE INDEX repair_idx ON repair_view(value)",
        ],
    );
    let mut retired: Option<(String, _)> = None;
    for refresh in [false, true] {
        if refresh {
            fixture
                .execute("REFRESH MATERIALIZED VIEW CONCURRENTLY repair_view")
                .unwrap();
        }
        let metadata = fixture
            .cassie
            .catalog
            .get_materialized_projection("repair_view")
            .unwrap();
        let output = metadata.active_output_collection().unwrap();
        let before = remove_entries(&fixture.cassie, output);
        fixture
            .execute("INSERT INTO repair_source VALUES(11)")
            .unwrap();
        let rows = fixture.rows("SELECT value FROM repair_view ORDER BY value");
        // Act
        fixture.execute("REINDEX TABLE repair_view").unwrap();
        // Assert
        assert_eq!(entries(&fixture.cassie, output), before);
        if let Some((ref old_output, ref old_entries)) = retired {
            assert_eq!(entries(&fixture.cassie, old_output), *old_entries);
        }
        retired = Some((output.to_string(), before));
        assert_eq!(
            fixture.rows("SELECT value FROM repair_view ORDER BY value"),
            rows
        );
        assert_eq!(
            fixture
                .cassie
                .catalog
                .get_materialized_projection("repair_view")
                .unwrap()
                .active_output_collection(),
            Some(output)
        );
    }
}

#[test]
fn should_preserve_unpopulated_materialized_state_during_reindex() {
    // Arrange
    let fixture = sql_fixture(
        "reindex-unpopulated",
        &["CREATE MATERIALIZED VIEW repair_empty AS SELECT 1 AS value WITH NO DATA"],
    );
    // Act
    fixture.execute("REINDEX TABLE repair_empty").unwrap();
    // Assert
    assert_eq!(
        fixture.rows("SELECT ispopulated FROM pg_matviews WHERE matviewname = 'repair_empty'"),
        vec![vec![Value::Bool(false)]]
    );
}

#[test]
fn should_recover_rebuilt_materialized_entries_after_local_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new("reindex-mv-restart");
    let (output, before) = {
        let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
            &directory.0,
            cassie::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE restart_source(value INT)",
            "INSERT INTO restart_source VALUES(7)",
            "CREATE MATERIALIZED VIEW restart_view AS SELECT value FROM restart_source",
            "CREATE INDEX restart_idx ON restart_view(value)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        let output = cassie
            .catalog
            .get_materialized_projection("restart_view")
            .unwrap()
            .active_output_collection()
            .unwrap()
            .to_string();
        let before = remove_entries(&cassie, &output);
        // Act
        cassie
            .execute_sql(&session, "REINDEX TABLE restart_view", vec![])
            .unwrap();
        (output, before)
    };
    let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
        &directory.0,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    // Assert
    assert_eq!(entries(&cassie, &output), before);
}
