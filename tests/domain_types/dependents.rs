//! Objects that depend on a domain: values inside row-type columns and
//! views whose definition casts to the domain.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

/// A domain used by `r1.a`, whose row type is the type of `r2.r`.
fn embedded(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN g AS int",
            "CREATE TABLE r1 (id int primary key, a g)",
            "CREATE TABLE r2 (id int primary key, r r1)",
            "INSERT INTO r2 VALUES (1, ROW(1, -5)), (2, ROW(2, NULL))",
        ],
    )
}

/// A domain that only the view `w1` casts to.
fn viewed(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN h AS int CHECK (VALUE > 0)",
            "CREATE VIEW w1 AS SELECT 5::h + 0 AS x",
        ],
    )
}

/// A domain used by a column and by a cast in the view `w2`.
fn renamed(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN m AS int CHECK (VALUE > 0)",
            "CREATE TABLE u1 (id int primary key, x m)",
            "INSERT INTO u1 VALUES (1, 2)",
            "CREATE VIEW w2 AS SELECT id, x::m + 0 AS y FROM u1",
        ],
    )
}

#[test]
fn should_reject_add_constraint_while_a_row_type_column_embeds_the_domain_with_sqlstate_0a000() {
    // Arrange
    let fixture = embedded("domain_embedded_add_check");

    // Act
    let error = fixture.error("ALTER DOMAIN g ADD CONSTRAINT gpos CHECK (VALUE > 0)");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_set_not_null_while_a_row_type_column_embeds_the_domain_with_sqlstate_0a000() {
    // Arrange
    let fixture = embedded("domain_embedded_set_not_null");

    // Act
    let error = fixture.error("ALTER DOMAIN g SET NOT NULL");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_leave_the_domain_nullable_after_a_rejected_set_not_null() {
    // Arrange
    let fixture = embedded("domain_embedded_unchanged");
    let rejected = fixture.execute("ALTER DOMAIN g SET NOT NULL");

    // Act
    let flag = fixture.rows("SELECT typnotnull FROM pg_type WHERE typname = 'g'");

    // Assert
    assert!(rejected.is_err(), "the alteration must be refused");
    assert_eq!(flag, vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_accept_a_not_valid_constraint_while_a_row_type_column_embeds_the_domain() {
    // Arrange
    let fixture = embedded("domain_embedded_not_valid");

    // Act
    let added = fixture.execute("ALTER DOMAIN g ADD CONSTRAINT gpos CHECK (VALUE > 0) NOT VALID");

    // Assert
    assert!(added.is_ok(), "NOT VALID does not inspect stored values");
}

#[test]
fn should_reject_validate_constraint_while_a_row_type_column_embeds_the_domain_with_sqlstate_0a000()
{
    // Arrange
    let fixture = embedded("domain_embedded_validate");
    fixture
        .execute("ALTER DOMAIN g ADD CONSTRAINT gpos CHECK (VALUE > 0) NOT VALID")
        .unwrap();

    // Act
    let error = fixture.error("ALTER DOMAIN g VALIDATE CONSTRAINT gpos");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_drop_domain_with_a_dependent_view_with_sqlstate_2bp01() {
    // Arrange
    let fixture = viewed("domain_view_drop_restrict");

    // Act
    let error = fixture.error("DROP DOMAIN h");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_keep_a_dependent_view_usable_after_a_rejected_drop_domain() {
    // Arrange
    let fixture = viewed("domain_view_drop_restrict_keeps");
    let rejected = fixture.execute("DROP DOMAIN h");

    // Act
    let rows = fixture.rows("SELECT x FROM w1");

    // Assert
    assert!(rejected.is_err(), "the drop must be refused");
    assert_eq!(rows, vec![vec![Value::Int64(5)]]);
}

#[test]
fn should_drop_a_dependent_view_with_drop_domain_cascade() {
    // Arrange
    let fixture = viewed("domain_view_drop_cascade");

    // Act
    let dropped = fixture.execute("DROP DOMAIN h CASCADE");
    let recreated = fixture.execute("CREATE VIEW w1 AS SELECT 1 AS x");

    // Assert
    assert!(dropped.is_ok(), "CASCADE removes the view with the domain");
    assert!(recreated.is_ok(), "the dependent view no longer exists");
}

#[test]
fn should_drop_a_view_over_a_dependent_view_with_drop_domain_cascade() {
    // Arrange
    let fixture = viewed("domain_view_drop_cascade_nested");
    fixture
        .execute("CREATE VIEW outer_view AS SELECT x FROM w1")
        .unwrap();

    // Act
    let dropped = fixture.execute("DROP DOMAIN h CASCADE");
    let recreated = fixture.execute("CREATE VIEW outer_view AS SELECT 1 AS x");

    // Assert
    assert!(
        dropped.is_ok(),
        "CASCADE reaches views over the dependent view"
    );
    assert!(recreated.is_ok(), "the outer view no longer exists");
}

#[test]
fn should_drop_an_outside_view_with_drop_schema_cascade() {
    // Arrange
    let fixture = sql_fixture(
        "domain_view_schema_cascade",
        &[
            "CREATE SCHEMA sa",
            "CREATE DOMAIN sa.da AS int",
            "CREATE VIEW w3 AS SELECT 5::sa.da + 0 AS x",
        ],
    );

    // Act
    let dropped = fixture.execute("DROP SCHEMA sa CASCADE");
    let recreated = fixture.execute("CREATE VIEW w3 AS SELECT 1 AS x");

    // Assert
    assert!(
        dropped.is_ok(),
        "CASCADE removes the schema and its dependents"
    );
    assert!(
        recreated.is_ok(),
        "the view that cast to the domain is gone"
    );
}

#[test]
fn should_keep_a_view_working_after_alter_domain_rename() {
    // Arrange
    let fixture = renamed("domain_view_rename");
    fixture.execute("ALTER DOMAIN m RENAME TO m2").unwrap();

    // Act
    let rows = fixture.rows("SELECT y FROM w2");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_keep_a_view_working_after_alter_domain_set_schema() {
    // Arrange
    let fixture = renamed("domain_view_set_schema");
    fixture.execute("CREATE SCHEMA sb").unwrap();
    fixture.execute("ALTER DOMAIN m SET SCHEMA sb").unwrap();

    // Act
    let rows = fixture.rows("SELECT y FROM w2");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_enforce_a_renamed_domain_through_a_view_cast() {
    // Arrange
    let fixture = renamed("domain_view_rename_enforces");
    fixture.execute("ALTER DOMAIN m RENAME TO m2").unwrap();
    fixture
        .execute("ALTER DOMAIN m2 ADD CONSTRAINT small CHECK (VALUE < 2) NOT VALID")
        .unwrap();

    // Act
    let error = fixture.error("SELECT y FROM w2");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}
