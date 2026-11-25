//! `ALTER TABLE ... RENAME TO` and `RENAME COLUMN` while a view reads the
//! table or the column. `ALTER COLUMN TYPE` under a view is refused with 0A000
//! as PostgreSQL does.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

struct ViewRenamePublicationFailureGuard;

impl Drop for ViewRenamePublicationFailureGuard {
    fn drop(&mut self) {
        cassie::midge::adapter::set_view_rename_publication_failure_point(false);
    }
}

fn base(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE rt (id INT, n INT)",
            "CREATE TABLE ot (id INT, n INT, m INT)",
            "INSERT INTO rt VALUES (1, 5)",
            "INSERT INTO ot VALUES (1, 7, 9)",
            view,
        ],
    )
}

fn view_rows(fixture: &SqlFixture) -> Vec<Vec<Value>> {
    fixture.rows("SELECT * FROM rv")
}

#[test]
fn should_rewrite_a_table_reference_in_a_dependent_view() {
    // Arrange
    let (label, view) = (
        "rename-table-view",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_validate_table_column_rewrites_in_the_view_function_scope() {
    // Arrange
    let fixture = sql_fixture(
        "rename-view-validation-function-scope",
        &[
            "CREATE SCHEMA validation_functions",
            "CREATE FUNCTION validation_functions.identity_value(x INT) RETURNS INT AS 'x'",
            "SET search_path = validation_functions, public",
            "CREATE TABLE validation_source (id INT)",
            "INSERT INTO validation_source VALUES (7)",
            "CREATE VIEW validation_view AS \
             SELECT identity_value(id) AS id_value FROM validation_source",
            "SET search_path = public",
        ],
    );

    // Act
    let column_rename =
        fixture.execute("ALTER TABLE validation_source RENAME COLUMN id TO next_id");
    let table_rename = fixture.execute("ALTER TABLE validation_source RENAME TO validation_next");
    let rows = fixture.rows("SELECT id_value FROM validation_view");

    // Assert
    assert!(column_rename.is_ok());
    assert!(table_rename.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_rewrite_a_table_reference_in_a_join_view() {
    // Arrange
    let (label, view) = (
        "rename-table-join",
        "CREATE VIEW rv AS SELECT rt.id, ot.m FROM rt JOIN ot ON rt.id = ot.id",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(9)]]);
}

#[test]
fn should_rewrite_a_table_reference_under_a_source_alias() {
    // Arrange
    let (label, view) = (
        "rename-table-alias",
        "CREATE VIEW rv AS SELECT x.id FROM rt AS x",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_rewrite_a_table_reference_in_a_subquery() {
    // Arrange
    let (label, view) = (
        "rename-table-subquery",
        "CREATE VIEW rv AS SELECT id FROM ot WHERE id IN (SELECT id FROM rt)",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_rewrite_a_table_reference_through_a_view_chain() {
    // Arrange
    let fixture = base(
        "rename-table-chain",
        "CREATE VIEW rv0 AS SELECT id, n FROM rt",
    );
    fixture
        .execute("CREATE VIEW rv AS SELECT id, n FROM rv0")
        .expect("the second view is created");

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_rewrite_a_schema_qualified_table_reference() {
    // Arrange
    let fixture = sql_fixture(
        "rename-table-qualified",
        &[
            "CREATE SCHEMA qs",
            "CREATE TABLE qs.rq (id INT)",
            "INSERT INTO qs.rq VALUES (1)",
            "CREATE VIEW rv AS SELECT id FROM qs.rq",
        ],
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE qs.rq RENAME TO rq2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok(), "{renamed:?}");
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_rewrite_a_column_reference_in_the_select_list() {
    // Arrange
    let (label, view) = (
        "rename-column-select",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_rewrite_a_column_reference_used_only_in_where() {
    // Arrange
    let (label, view) = (
        "rename-column-where",
        "CREATE VIEW rv AS SELECT id FROM rt WHERE n > 1",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_rewrite_a_column_qualified_by_table_name() {
    // Arrange
    let (label, view) = (
        "rename-column-qualified",
        "CREATE VIEW rv AS SELECT rt.id, rt.n FROM rt",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_rewrite_a_column_qualified_by_source_alias() {
    // Arrange
    let (label, view) = (
        "rename-column-alias",
        "CREATE VIEW rv AS SELECT x.n FROM rt x",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let result = fixture.execute("SELECT * FROM rv").expect("the view reads");
    let rows = result.rows().to_vec();
    let columns = result.columns();

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(columns[0].name, "n");
    assert_eq!(columns[0].type_oid, cassie::types::DataType::Int.type_oid());
}

#[test]
fn should_preserve_a_wildcard_view_output_after_column_rename() {
    // Arrange
    let (label, view) = ("rename-column-star", "CREATE VIEW rv AS SELECT * FROM rt");

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_allow_renaming_a_column_another_table_in_the_view_owns() {
    // Arrange
    let fixture = base(
        "rename-column-other-table",
        "CREATE VIEW rv AS SELECT rt.id, ot.n FROM rt JOIN ot ON rt.id = ot.id",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO k");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok(), "the view never reads rt.n");
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(7)]]);
}

#[test]
fn should_allow_renaming_a_table_no_view_reads() {
    // Arrange
    let fixture = base(
        "rename-table-unrelated",
        "CREATE VIEW rv AS SELECT id, m FROM ot",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok(), "no view reads rt");
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(9)]]);
}

#[test]
fn should_preserve_results_through_a_dependent_view_chain_after_table_rename() {
    // Arrange
    let fixture = base(
        "rename-table-view-chain",
        "CREATE VIEW rv0 AS SELECT id, n FROM rt",
    );
    fixture
        .execute("CREATE VIEW rv AS SELECT id, n FROM rv0")
        .expect("the dependent view is created");

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(
        renamed.is_ok(),
        "the stored relation reference is rewritten"
    );
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_preserve_the_projected_column_name_type_after_column_rename() {
    // Arrange
    let fixture = base(
        "rename-column-output-contract",
        "CREATE VIEW rv AS SELECT n FROM rt",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);
    let field = &fixture
        .cassie
        .catalog
        .get_view("rv")
        .expect("view metadata")
        .schema
        .fields[0];

    // Assert
    assert!(renamed.is_ok(), "the stored column reference is rewritten");
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(field.name, "n");
    assert_eq!(field.data_type, cassie::types::DataType::Int);
}

#[test]
fn should_rewrite_a_column_reference_in_a_view_predicate() {
    // Arrange
    let fixture = base(
        "rename-column-predicate",
        "CREATE VIEW rv AS SELECT id FROM rt WHERE n > 4",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok(), "the stored predicate is rewritten");
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_preserve_string_literals_during_column_rewrite() {
    // Arrange
    let fixture = base(
        "rename-column-literal",
        "CREATE VIEW rv AS SELECT 'n' AS label, n FROM rt",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);
    let stored = fixture
        .cassie
        .catalog
        .get_view("rv")
        .expect("view metadata");

    // Assert
    assert!(renamed.is_ok(), "the stored column reference is rewritten");
    assert_eq!(rows, vec![vec![Value::String("n".into()), Value::Int64(5)]]);
    assert!(stored.query.contains("'n'"));
}

#[test]
fn should_restore_view_table_names_after_transactional_rename_rollback() {
    // Arrange
    let fixture = base("rename-rollback", "CREATE VIEW rv AS SELECT id, n FROM rt");
    fixture.execute("BEGIN").expect("begin");

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    fixture.execute("ROLLBACK").expect("rollback");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_rollback_owned_implicit_transaction_after_rename_error() {
    // Arrange
    let fixture = base(
        "rename-implicit-rollback",
        "CREATE VIEW rv AS SELECT id FROM rt",
    );

    // Act
    let rename_error = fixture.error("ALTER TABLE rt RENAME TO ot");
    fixture
        .execute("CREATE TABLE after_failure (id INT)")
        .expect("the next statement starts after rename cleanup");
    fixture
        .execute("INSERT INTO after_failure VALUES (7)")
        .expect("the follow-up row is inserted");
    let observer = fixture.cassie.create_session("root", None);
    let visible = fixture
        .cassie
        .execute_sql(&observer, "SELECT id FROM after_failure", vec![])
        .expect("another session reads the follow-up table")
        .rows()
        .to_vec();

    // Assert
    assert_ne!(rename_error.to_string(), "");
    assert_eq!(visible, vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_refuse_changing_the_type_of_a_column_a_view_uses() {
    // Arrange
    let fixture = base("alter-type-view", "CREATE VIEW rv AS SELECT id, n FROM rt");

    // Act
    let sqlstate = fixture
        .error("ALTER TABLE rt ALTER COLUMN n TYPE TEXT USING n::text")
        .sql_state()
        .to_owned();
    let rows = view_rows(&fixture);

    // Assert
    assert_eq!(sqlstate, "0A000");
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_allow_changing_the_type_of_a_column_no_view_uses() {
    // Arrange
    let fixture = base("alter-type-unused", "CREATE VIEW rv AS SELECT id FROM rt");

    // Act
    let altered = fixture.execute("ALTER TABLE rt ALTER COLUMN n TYPE TEXT USING n::text");

    // Assert
    assert!(altered.is_ok(), "the view never reads rt.n");
}

#[test]
fn should_rewrite_a_table_reference_in_a_union_branch() {
    // Arrange
    let fixture = base(
        "rename-table-union",
        "CREATE VIEW rv AS SELECT id, n FROM rt UNION ALL SELECT id, m FROM ot",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows.len(), 2, "both union branches still resolve");
}

#[test]
fn should_rewrite_a_table_reference_in_a_scalar_subquery() {
    // Arrange
    let (label, view) = (
        "rename-table-scalar",
        "CREATE VIEW rv AS SELECT id, (SELECT MAX(n) FROM rt) AS n FROM ot",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_rewrite_a_table_reference_in_a_cte() {
    // Arrange
    let (label, view) = (
        "rename-table-cte",
        "CREATE VIEW rv AS WITH c AS (SELECT id, n FROM rt) SELECT id, n FROM c",
    );

    // Act
    let fixture = base(label, view);
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
}

#[test]
fn should_publish_cross_schema_view_rewrite_with_table_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-table-cross-schema-view",
        &[
            "CREATE SCHEMA rename_source",
            "CREATE SCHEMA rename_view",
            "CREATE TABLE rename_source.rename_before (n INT)",
            "INSERT INTO rename_source.rename_before VALUES (17)",
            "CREATE VIEW rename_view.rename_result AS \
             SELECT n FROM rename_source.rename_before",
        ],
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rename_source.rename_before RENAME TO rename_after");
    let rows = fixture.rows("SELECT n FROM rename_view.rename_result");
    let stored = fixture
        .cassie
        .catalog
        .get_view("rename_view.rename_result")
        .expect("view metadata");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(17)]]);
    assert!(stored.query.contains("rename_after"));
    assert!(!stored.query.contains("rename_before"));
}

#[test]
fn should_publish_cross_schema_view_rewrite_with_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-column-cross-schema-view",
        &[
            "CREATE SCHEMA rename_source",
            "CREATE SCHEMA rename_view",
            "CREATE TABLE rename_source.rename_values (n INT)",
            "INSERT INTO rename_source.rename_values VALUES (23)",
            "CREATE VIEW rename_view.rename_result AS \
             SELECT n FROM rename_source.rename_values",
        ],
    );

    // Act
    let renamed =
        fixture.execute("ALTER TABLE rename_source.rename_values RENAME COLUMN n TO renamed_n");
    let rows = fixture.rows("SELECT n FROM rename_view.rename_result");
    let stored = fixture
        .cassie
        .catalog
        .get_view("rename_view.rename_result")
        .expect("view metadata");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec![vec![Value::Int64(23)]]);
    assert!(stored.query.contains("renamed_n"));
}

#[test]
fn should_restore_renamed_relation_view_state_after_local_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("table-rename-dependent-view-restart");
    let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("create initial Cassie");
    cassie.startup().expect("start initial Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE restart_before (n INT)",
        "INSERT INTO restart_before VALUES (29)",
        "CREATE VIEW restart_view AS SELECT n FROM restart_before",
        "ALTER TABLE restart_before RENAME TO restart_after",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    }
    cassie.shutdown();
    drop(session);
    drop(cassie);

    // Act
    let reopened = cassie::app::Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    reopened.startup().expect("recover rename publication");
    let reopened_session = reopened.create_session("tester", None);
    let rows = reopened
        .execute_sql(&reopened_session, "SELECT n FROM restart_view", vec![])
        .expect("read rewritten view after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(29)]]);
    reopened.shutdown();
}

#[test]
fn should_not_partially_publish_table_or_view_for_target_collision() {
    // Arrange
    let fixture = base(
        "rename-table-target-collision",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );

    // Act
    let failure = fixture.error("ALTER TABLE rt RENAME TO ot");
    let old_rows = view_rows(&fixture);
    let new_visible = fixture.execute("SELECT id, n FROM rt");

    // Assert
    assert_eq!(failure.sql_state(), "42P07");
    assert_eq!(old_rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
    assert!(new_visible.is_ok(), "the source table remains published");
    assert!(fixture.cassie.catalog.get_view("rv").is_some());
}

#[test]
fn should_not_publish_table_or_view_at_an_injected_table_rename_boundary() {
    // Arrange
    let fixture = base(
        "rename-table-view-publication-failure",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );
    cassie::midge::adapter::set_view_rename_publication_failure_point(true);
    let _guard = ViewRenamePublicationFailureGuard;

    // Act
    let failed = fixture.error("ALTER TABLE rt RENAME TO rt2");
    let source_rows = fixture.rows("SELECT id, n FROM rt");
    let view_rows = view_rows(&fixture);
    let stored = fixture
        .cassie
        .catalog
        .get_view("rv")
        .expect("view metadata")
        .query;

    // Assert
    assert_eq!(failed.sql_state(), "22000");
    assert_eq!(source_rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
    assert_eq!(view_rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
    assert!(stored.contains("rt"));
    assert!(!stored.contains("rt2"));
}

#[test]
fn should_not_publish_column_or_view_at_an_injected_column_rename_boundary() {
    // Arrange
    let fixture = base(
        "rename-column-view-publication-failure",
        "CREATE VIEW rv AS SELECT n FROM rt",
    );
    cassie::midge::adapter::set_view_rename_publication_failure_point(true);
    let _guard = ViewRenamePublicationFailureGuard;

    // Act
    let failed = fixture.error("ALTER TABLE rt RENAME COLUMN n TO m");
    let source_rows = fixture.rows("SELECT n FROM rt");
    let view_rows = view_rows(&fixture);
    let stored = fixture
        .cassie
        .catalog
        .get_view("rv")
        .expect("view metadata")
        .query;

    // Assert
    assert_eq!(failed.sql_state(), "22000");
    assert_eq!(source_rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(view_rows, vec![vec![Value::Int64(5)]]);
    assert!(stored.contains('n'));
    assert!(!stored.contains('m'));
}

#[test]
fn should_rewrite_a_view_when_a_table_moves_schema() {
    // Arrange
    let fixture = sql_fixture(
        "rename-table-set-schema-view",
        &[
            "CREATE SCHEMA move_source",
            "CREATE SCHEMA move_target",
            "CREATE TABLE move_source.before_move (n INT)",
            "INSERT INTO move_source.before_move VALUES (31)",
            "CREATE VIEW external_move_view AS SELECT n FROM move_source.before_move",
        ],
    );

    // Act
    let moved = fixture.execute("ALTER TABLE move_source.before_move SET SCHEMA move_target");
    let rows = fixture.rows("SELECT n FROM external_move_view");
    let stored = fixture
        .cassie
        .catalog
        .get_view("external_move_view")
        .expect("view metadata")
        .query;

    // Assert
    assert!(moved.is_ok(), "{moved:?}");
    assert_eq!(rows, vec![vec![Value::Int64(31)]]);
    assert!(stored.contains("move_target.before_move"));
    assert!(!stored.contains("move_source.before_move"));
}

#[test]
fn should_recover_transactional_table_rename_view_rollback_after_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path =
        crate::support_sql::data_dir("table-rename-dependent-view-transaction-rollback-restart");
    let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("create initial Cassie");
    cassie.startup().expect("start initial Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE restart_source (n INT)",
        "INSERT INTO restart_source VALUES (37)",
        "CREATE VIEW restart_rollback_view AS SELECT n FROM restart_source",
        "BEGIN",
        "ALTER TABLE restart_source RENAME TO restart_destination",
        "ROLLBACK",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    }
    cassie.shutdown();
    drop(session);
    drop(cassie);

    // Act
    let reopened = cassie::app::Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    reopened.startup().expect("recover transactional rollback");
    let reopened_session = reopened.create_session("tester", None);
    let source_rows = reopened
        .execute_sql(&reopened_session, "SELECT n FROM restart_source", vec![])
        .expect("read source after restart")
        .rows()
        .to_vec();
    let view_rows = reopened
        .execute_sql(
            &reopened_session,
            "SELECT n FROM restart_rollback_view",
            vec![],
        )
        .expect("read view after restart")
        .rows()
        .to_vec();
    let destination_exists = reopened
        .execute_sql(
            &reopened_session,
            "SELECT n FROM restart_destination",
            vec![],
        )
        .is_err();

    // Assert
    assert_eq!(source_rows, vec![vec![Value::Int64(37)]]);
    assert_eq!(view_rows, vec![vec![Value::Int64(37)]]);
    assert!(
        destination_exists,
        "the rolled-back destination stayed unpublished"
    );
    reopened.shutdown();
}
