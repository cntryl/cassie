//! `ALTER TABLE ... RENAME TO` and `RENAME COLUMN` while a view reads the
//! table or the column. `ALTER COLUMN TYPE` under a view
//! view is refused with 0A000 as PostgreSQL does.

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

fn rename_outcome(label: &str, view: &str, statement: &str) -> (String, usize) {
    let fixture = base(label, view);
    let sqlstate = fixture.error(statement).sql_state().to_owned();
    (sqlstate, view_rows(&fixture).len())
}

#[test]
fn should_refuse_renaming_a_table_a_view_reads() {
    // Arrange
    let (label, view) = (
        "rename-table-view",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_table_a_join_view_reads() {
    // Arrange
    let (label, view) = (
        "rename-table-join",
        "CREATE VIEW rv AS SELECT rt.id, ot.m FROM rt JOIN ot ON rt.id = ot.id",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_table_a_view_reads_through_an_alias() {
    // Arrange
    let (label, view) = (
        "rename-table-alias",
        "CREATE VIEW rv AS SELECT x.id FROM rt AS x",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_table_a_view_reads_in_a_subquery() {
    // Arrange
    let (label, view) = (
        "rename-table-subquery",
        "CREATE VIEW rv AS SELECT id FROM ot WHERE id IN (SELECT id FROM rt)",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_table_a_view_over_a_view_reads() {
    // Arrange
    let fixture = base(
        "rename-table-chain",
        "CREATE VIEW rv0 AS SELECT id, n FROM rt",
    );
    fixture
        .execute("CREATE VIEW rv AS SELECT id, n FROM rv0")
        .expect("the second view is created");

    // Act
    let sqlstate = fixture
        .error("ALTER TABLE rt RENAME TO rt2")
        .sql_state()
        .to_owned();
    let rows = view_rows(&fixture);

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(rows.len(), 1);
}

#[test]
fn should_refuse_renaming_a_table_a_schema_qualified_view_reads() {
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
    let sqlstate = fixture
        .error("ALTER TABLE qs.rq RENAME TO rq2")
        .sql_state()
        .to_owned();
    let rows = view_rows(&fixture);

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_refuse_renaming_a_column_in_the_select_list() {
    // Arrange
    let (label, view) = (
        "rename-column-select",
        "CREATE VIEW rv AS SELECT id, n FROM rt",
    );

    // Act
    let (sqlstate, visible_rows) =
        rename_outcome(label, view, "ALTER TABLE rt RENAME COLUMN n TO m");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_column_used_only_in_where() {
    // Arrange
    let (label, view) = (
        "rename-column-where",
        "CREATE VIEW rv AS SELECT id FROM rt WHERE n > 1",
    );

    // Act
    let (sqlstate, visible_rows) =
        rename_outcome(label, view, "ALTER TABLE rt RENAME COLUMN n TO m");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_column_qualified_by_table_name() {
    // Arrange
    let (label, view) = (
        "rename-column-qualified",
        "CREATE VIEW rv AS SELECT rt.id, rt.n FROM rt",
    );

    // Act
    let (sqlstate, visible_rows) =
        rename_outcome(label, view, "ALTER TABLE rt RENAME COLUMN n TO m");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_column_qualified_by_alias() {
    // Arrange
    let (label, view) = (
        "rename-column-alias",
        "CREATE VIEW rv AS SELECT x.id, x.n FROM rt x",
    );

    // Act
    let (sqlstate, visible_rows) =
        rename_outcome(label, view, "ALTER TABLE rt RENAME COLUMN n TO m");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_column_a_star_view_exposes() {
    // Arrange
    let (label, view) = ("rename-column-star", "CREATE VIEW rv AS SELECT * FROM rt");

    // Act
    let (sqlstate, visible_rows) =
        rename_outcome(label, view, "ALTER TABLE rt RENAME COLUMN n TO m");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
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
fn should_preserve_aliased_join_chain_results_after_table_rename() {
    // Arrange
    let fixture = base(
        "rename-table-aliased-join-chain",
        "CREATE VIEW rv0 AS SELECT x.id AS source_id, ot.m FROM rt AS x JOIN ot ON x.id = ot.id",
    );
    fixture
        .execute("CREATE VIEW rv AS SELECT source_id, m FROM rv0")
        .expect("the dependent view is created");

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME TO rt2");
    let rows = view_rows(&fixture);

    // Assert
    assert!(renamed.is_ok(), "the stored relation reference is rewritten");
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(9)]]);
}

#[test]
fn should_preserve_the_projected_column_name_and_type_after_column_rename() {
    // Arrange
    let fixture = base("rename-column-output-contract", "CREATE VIEW rv AS SELECT n FROM rt");

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);
    let field = &fixture.cassie.catalog.get_view("rv").expect("view metadata").schema.fields[0];

    // Assert
    assert!(renamed.is_ok(), "the stored column reference is rewritten");
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert_eq!(field.name, "n");
    assert_eq!(field.data_type, cassie::types::DataType::Int);
}

#[test]
fn should_rewrite_column_predicates_without_changing_literal_text() {
    // Arrange
    let fixture = base(
        "rename-column-predicate-literal",
        "CREATE VIEW rv AS SELECT n AS value FROM rt WHERE n > 4 AND 'n' = 'n'",
    );

    // Act
    let renamed = fixture.execute("ALTER TABLE rt RENAME COLUMN n TO m");
    let rows = view_rows(&fixture);
    let stored = fixture.cassie.catalog.get_view("rv").expect("view metadata");

    // Assert
    assert!(renamed.is_ok(), "the stored predicate and projection are rewritten");
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
    assert!(stored.query.contains("'n' = 'n'"));
}

#[test]
fn should_keep_the_view_working_after_a_refused_rename_is_rolled_back() {
    // Arrange
    let fixture = base("rename-rollback", "CREATE VIEW rv AS SELECT id, n FROM rt");
    fixture.execute("BEGIN").expect("begin");

    // Act
    let sqlstate = fixture
        .error("ALTER TABLE rt RENAME TO rt2")
        .sql_state()
        .to_owned();
    fixture.execute("ROLLBACK").expect("rollback");
    let rows = view_rows(&fixture);

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(5)]]);
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
fn should_refuse_renaming_a_table_a_union_branch_reads() {
    // Arrange
    let fixture = base(
        "rename-table-union",
        "CREATE VIEW rv AS SELECT id, n FROM rt UNION ALL SELECT id, m FROM ot",
    );

    // Act
    let sqlstate = fixture
        .error("ALTER TABLE rt RENAME TO rt2")
        .sql_state()
        .to_owned();
    let rows = view_rows(&fixture);

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(rows.len(), 2, "both union branches still resolve");
}

#[test]
fn should_refuse_renaming_a_table_a_scalar_subquery_reads() {
    // Arrange
    let (label, view) = (
        "rename-table-scalar",
        "CREATE VIEW rv AS SELECT id, (SELECT MAX(n) FROM rt) AS n FROM ot",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}

#[test]
fn should_refuse_renaming_a_table_a_cte_reads() {
    // Arrange
    let (label, view) = (
        "rename-table-cte",
        "CREATE VIEW rv AS WITH c AS (SELECT id, n FROM rt) SELECT id, n FROM c",
    );

    // Act
    let (sqlstate, visible_rows) = rename_outcome(label, view, "ALTER TABLE rt RENAME TO rt2");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(visible_rows, 1, "the view still resolves");
}
