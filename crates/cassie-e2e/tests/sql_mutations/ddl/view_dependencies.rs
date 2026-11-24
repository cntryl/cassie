//! Dependent views are found from the parsed view definition, so `DROP VIEW`,
//! `DROP TABLE` and `ALTER TABLE ... DROP COLUMN` agree with PostgreSQL 18.6
//! (SQLSTATE 2BP01) however the view spells its sources and column references.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn relation_count(fixture: &SqlFixture, names: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT count(*) FROM pg_class WHERE relname IN ({names})"
    ))
}

fn chain_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE b3 (id INT PRIMARY KEY)",
            "CREATE VIEW dv1 AS SELECT * FROM b3",
            "CREATE VIEW dv2 AS SELECT * FROM dv1",
        ],
    )
}

#[test]
fn should_reject_dropping_a_view_another_view_reads() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view");

    // Act
    let sqlstate = fixture.error("DROP VIEW dv1").sql_state().to_owned();
    let remaining = relation_count(&fixture, "'dv1', 'dv2'");

    // Assert
    assert_eq!(sqlstate, "2BP01");
    assert_eq!(remaining, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_drop_a_view_list_that_contains_the_dependent_view() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view-list");

    // Act
    let dropped = fixture.execute("DROP VIEW dv1, dv2");
    let remaining = relation_count(&fixture, "'dv1', 'dv2'");

    // Assert
    assert!(dropped.is_ok(), "both views go in one statement");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_drop_every_view_in_the_chain_with_drop_view_cascade() {
    // Arrange
    let fixture = chain_fixture("view-dep-drop-view-cascade");
    fixture
        .execute("CREATE VIEW dv3 AS SELECT * FROM dv2")
        .expect("the third view is created");

    // Act
    let dropped = fixture.execute("DROP VIEW dv1 CASCADE");
    let remaining = relation_count(&fixture, "'dv1', 'dv2', 'dv3'");
    let table_rows = fixture.rows("SELECT count(*) FROM b3");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the dependent views");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
    assert_eq!(table_rows, vec![vec![Value::Int64(0)]]);
}

fn two_table_fixture(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE va (id INT PRIMARY KEY)",
            "CREATE TABLE vt (id INT PRIMARY KEY)",
            view,
        ],
    )
}

fn drop_table_sqlstate(label: &str, view: &str) -> String {
    two_table_fixture(label, view)
        .error("DROP TABLE vt")
        .sql_state()
        .to_owned()
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_through_a_comma_join() {
    // Arrange
    let view = "CREATE VIEW vc AS SELECT va.id AS a_id, vt.id AS t_id FROM va, vt";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-comma-join", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_in_a_subquery() {
    // Arrange
    let view = "CREATE VIEW vs AS SELECT id FROM va WHERE id IN (SELECT id FROM vt)";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-subquery", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_in_a_cte() {
    // Arrange
    let view = "CREATE VIEW vw AS WITH c AS (SELECT id FROM vt) SELECT id FROM c";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-cte", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_table_a_view_reads_through_an_explicit_join() {
    // Arrange
    let view = "CREATE VIEW vj AS SELECT va.id FROM va LEFT JOIN vt ON va.id = vt.id";

    // Act
    let sqlstate = drop_table_sqlstate("view-dep-explicit-join", view);

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_drop_the_whole_view_chain_with_drop_table_cascade() {
    // Arrange
    let fixture = sql_fixture(
        "view-dep-table-cascade-chain",
        &[
            "CREATE TABLE t (id INT PRIMARY KEY)",
            "CREATE VIEW cv1 AS SELECT id FROM t",
            "CREATE VIEW cv2 AS SELECT id FROM cv1",
        ],
    );

    // Act
    let dropped = fixture.execute("DROP TABLE t CASCADE");
    let remaining = relation_count(&fixture, "'t', 'cv1', 'cv2'");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the table");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}

fn column_fixture(label: &str, view: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE tc (id INT PRIMARY KEY, val INT, other INT)",
            "CREATE TABLE tz (id INT PRIMARY KEY, val INT)",
            view,
        ],
    )
}

fn drop_column_sqlstate(label: &str, view: &str, column: &str) -> String {
    column_fixture(label, view)
        .error(&format!("ALTER TABLE tc DROP COLUMN {column}"))
        .sql_state()
        .to_owned()
}

#[test]
fn should_reject_dropping_a_column_a_view_filters_on() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT id FROM tc WHERE val>0";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-filter", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_selects_with_a_table_qualifier() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT tc.other FROM tc";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-qualified", view, "other");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_selects_through_a_wildcard() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT * FROM tc";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-wildcard", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_uses_through_an_alias() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT x.id FROM tc AS x WHERE x.val IS NOT NULL";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-alias", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_reject_dropping_a_column_a_view_uses_in_a_correlated_subquery() {
    // Arrange
    let view = "CREATE VIEW vcol AS SELECT id FROM tz \
                WHERE EXISTS (SELECT 1 FROM tc WHERE tc.val = tz.id)";

    // Act
    let sqlstate = drop_column_sqlstate("view-dep-column-subquery", view, "val");

    // Assert
    assert_eq!(sqlstate, "2BP01");
}

#[test]
fn should_drop_a_column_a_view_only_names_in_a_string_literal() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-literal",
        "CREATE VIEW vcol AS SELECT id, 'val' AS label FROM tc",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(
        dropped.is_ok(),
        "a string literal is not a column reference"
    );
}

#[test]
fn should_drop_a_column_a_view_only_names_as_an_output_alias() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-output-alias",
        "CREATE VIEW vcol AS SELECT id AS val FROM tc ORDER BY val",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(dropped.is_ok(), "an output alias is not a column reference");
}

#[test]
fn should_drop_a_column_a_view_reads_from_another_table() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-other-table",
        "CREATE VIEW vcol AS SELECT tc.id, tz.val FROM tc, tz",
    );

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val");

    // Assert
    assert!(dropped.is_ok(), "tz.val is not a column of tc");
}

#[test]
fn should_drop_the_view_chain_with_drop_column_cascade() {
    // Arrange
    let fixture = column_fixture(
        "view-dep-column-cascade-chain",
        "CREATE VIEW cv1 AS SELECT id, val FROM tc",
    );
    fixture
        .execute("CREATE VIEW cv2 AS SELECT id FROM cv1")
        .expect("the second view is created");

    // Act
    let dropped = fixture.execute("ALTER TABLE tc DROP COLUMN val CASCADE");
    let remaining = relation_count(&fixture, "'cv1', 'cv2'");

    // Assert
    assert!(dropped.is_ok(), "CASCADE drops the column");
    assert_eq!(remaining, vec![vec![Value::Int64(0)]]);
}
