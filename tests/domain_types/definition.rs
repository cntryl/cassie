//! What a domain definition admits: CHECK expressions, constraint names and
//! the declared type a `CREATE TABLE AS` column inherits.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn empty(label: &str) -> SqlFixture {
    sql_fixture(label, &[])
}

/// A table with a column of the domain `p`.
fn priced(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE DOMAIN p AS int CHECK (VALUE > 0)",
            "CREATE TABLE c1 (id int, p p)",
            "INSERT INTO c1 VALUES (1, 5)",
        ],
    )
}

#[test]
fn should_reject_a_non_boolean_value_check_with_sqlstate_42804() {
    // Arrange
    let fixture = empty("domain_check_value_not_boolean");

    // Act
    let error = fixture.error("CREATE DOMAIN b1 AS int CHECK (VALUE)");

    // Assert
    assert_eq!(error.sql_state(), "42804");
}

#[test]
fn should_reject_an_arithmetic_check_with_sqlstate_42804() {
    // Arrange
    let fixture = empty("domain_check_arithmetic");

    // Act
    let error = fixture.error("CREATE DOMAIN b2 AS int CHECK (VALUE + 1)");

    // Assert
    assert_eq!(error.sql_state(), "42804");
}

#[test]
fn should_reject_a_check_calling_an_unknown_function_with_sqlstate_42883() {
    // Arrange
    let fixture = empty("domain_check_unknown_function");

    // Act
    let error = fixture.error("CREATE DOMAIN b3 AS int CHECK (nofunc(VALUE) > 1)");

    // Assert
    assert_eq!(error.sql_state(), "42883");
}

#[test]
fn should_reject_an_aggregate_in_a_check_with_sqlstate_42803() {
    // Arrange
    let fixture = empty("domain_check_aggregate");

    // Act
    let error = fixture.error("CREATE DOMAIN b4 AS int CHECK (count(VALUE) > 0)");

    // Assert
    assert_eq!(error.sql_state(), "42803");
}

#[test]
fn should_reject_a_non_boolean_check_added_by_alter_domain_with_sqlstate_42804() {
    // Arrange
    let fixture = sql_fixture(
        "domain_check_alter_not_boolean",
        &["CREATE DOMAIN b5 AS int"],
    );

    // Act
    let error = fixture.error("ALTER DOMAIN b5 ADD CONSTRAINT z CHECK (VALUE + 1)");

    // Assert
    assert_eq!(error.sql_state(), "42804");
}

#[test]
fn should_not_create_a_domain_whose_check_is_rejected() {
    // Arrange
    let fixture = empty("domain_check_rejected_publishes_nothing");
    let rejected = fixture.execute("CREATE DOMAIN b1 AS int CHECK (VALUE)");

    // Act
    let created = fixture.execute("CREATE DOMAIN b1 AS int CHECK (VALUE > 0)");

    // Assert
    assert!(rejected.is_err(), "the invalid definition must be refused");
    assert!(created.is_ok(), "the name must still be free");
}

#[test]
fn should_accept_a_bare_value_check_on_a_boolean_domain() {
    // Arrange
    let fixture = empty("domain_check_boolean_value");

    // Act
    let created = fixture.execute("CREATE DOMAIN b8 AS boolean CHECK (VALUE)");

    // Assert
    assert!(
        created.is_ok(),
        "VALUE of a boolean domain is a boolean CHECK"
    );
}

#[test]
fn should_accept_a_check_calling_a_builtin_function() {
    // Arrange
    let fixture = empty("domain_check_builtin_function");

    // Act
    let created = fixture.execute("CREATE DOMAIN b7 AS text CHECK (length(VALUE) < 5)");

    // Assert
    assert!(created.is_ok(), "a CHECK over a built-in function is valid");
}

#[test]
fn should_treat_quoted_constraint_names_as_case_sensitive() {
    // Arrange
    let fixture = empty("domain_constraint_case_distinct");

    // Act
    let created = fixture.execute(
        "CREATE DOMAIN cc AS int CONSTRAINT \"A\" CHECK (VALUE > 0) CONSTRAINT a CHECK (VALUE < 9)",
    );

    // Assert
    assert!(created.is_ok(), "\"A\" and a are different constraints");
}

#[test]
fn should_not_drop_a_quoted_constraint_by_its_folded_name() {
    // Arrange
    let fixture = sql_fixture(
        "domain_constraint_case_drop",
        &["CREATE DOMAIN cd AS int CONSTRAINT \"Ab\" CHECK (VALUE > 0)"],
    );

    // Act
    let error = fixture.error("ALTER DOMAIN cd DROP CONSTRAINT ab");

    // Assert
    assert_eq!(error.sql_state(), "42704");
}

#[test]
fn should_keep_the_domain_of_a_column_selected_by_create_table_as() {
    // Arrange
    let fixture = priced("domain_ctas_column");
    fixture
        .execute("CREATE TABLE c2 AS SELECT p FROM c1")
        .unwrap();

    // Act
    let error = fixture.error("INSERT INTO c2 VALUES (-1)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_keep_the_domain_of_a_renamed_column_selected_by_create_table_as() {
    // Arrange
    let fixture = priced("domain_ctas_alias");
    fixture
        .execute("CREATE TABLE c3 AS SELECT p AS renamed FROM c1")
        .unwrap();

    // Act
    let error = fixture.error("INSERT INTO c3 VALUES (-1)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_keep_the_domain_of_a_wildcard_column_selected_by_create_table_as() {
    // Arrange
    let fixture = priced("domain_ctas_wildcard");
    fixture
        .execute("CREATE TABLE c4 AS SELECT * FROM c1")
        .unwrap();

    // Act
    let error = fixture.error("INSERT INTO c4 (id, p) VALUES (2, -1)");

    // Assert
    assert_eq!(error.sql_state(), "23514");
}

#[test]
fn should_give_a_computed_create_table_as_column_the_base_type() {
    // Arrange
    let fixture = priced("domain_ctas_computed");
    fixture
        .execute("CREATE TABLE c5 AS SELECT p + 1 AS q FROM c1")
        .unwrap();

    // Act
    let inserted = fixture.execute("INSERT INTO c5 VALUES (-1)");

    // Assert
    assert!(inserted.is_ok(), "a computed column is not of the domain");
}
