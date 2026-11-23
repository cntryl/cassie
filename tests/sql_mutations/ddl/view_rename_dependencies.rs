//! `ALTER TABLE ... RENAME TO` and `RENAME COLUMN` while a view reads the
//! table or the column. `ALTER COLUMN TYPE` under a view is refused with 0A000
//! as PostgreSQL does.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

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
fn should_validate_table_and_column_rewrites_in_the_view_function_scope() {
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
    assert!(renamed.is_ok());
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
fn should_preserve_the_projected_column_name_and_type_after_column_rename() {
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
fn should_restore_view_and_table_names_after_transactional_rename_rollback() {
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
    assert!(!rename_error.to_string().is_empty());
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
