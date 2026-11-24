//! RENAME COLUMN rewrites every stored expression that names the column.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_keep_partial_unique_index_enforced_after_predicate_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-partial-unique",
        &[
            "CREATE TABLE pu (id INT, email TEXT, status TEXT)",
            "CREATE UNIQUE INDEX pu_ix ON pu (email) WHERE status = 'open'",
            "INSERT INTO pu VALUES (1, 'a@x', 'open')",
            "ALTER TABLE pu RENAME COLUMN status TO state",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO pu VALUES (2, 'a@x', 'open')");
    let outside_predicate = fixture.execute("INSERT INTO pu VALUES (3, 'a@x', 'closed')");

    // Assert
    assert!(
        duplicate.is_err(),
        "duplicate inside the predicate must fail"
    );
    assert!(
        outside_predicate.is_ok(),
        "row outside the predicate is allowed"
    );
}

#[test]
fn should_keep_expression_unique_index_enforced_after_operand_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-expression-unique",
        &[
            "CREATE TABLE eu (id INT, email TEXT)",
            "CREATE UNIQUE INDEX eu_ix ON eu (lower(email))",
            "INSERT INTO eu VALUES (1, 'a@x')",
            "ALTER TABLE eu RENAME COLUMN email TO mail",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO eu VALUES (2, 'A@X')");

    // Assert
    assert!(duplicate.is_err(), "case-insensitive duplicate must fail");
}

#[test]
fn should_not_rewrite_string_literal_matching_renamed_column_in_predicate() {
    // Arrange
    let fixture = sql_fixture(
        "rename-predicate-literal",
        &[
            "CREATE TABLE lit (id INT, k TEXT, a TEXT, ab TEXT)",
            "CREATE UNIQUE INDEX lit_ix ON lit (k) WHERE a = 'a' AND ab = 'a'",
            "INSERT INTO lit VALUES (1, 'x', 'a', 'a')",
            "ALTER TABLE lit RENAME COLUMN a TO renamed",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO lit VALUES (2, 'x', 'a', 'a')");
    let other_ab = fixture.execute("INSERT INTO lit VALUES (3, 'x', 'a', 'b')");

    // Assert
    assert!(
        duplicate.is_err(),
        "duplicate matching both terms must fail"
    );
    assert!(other_ab.is_ok(), "ab column must keep its own name");
}

#[test]
fn should_follow_rename_in_covering_include_column_of_index() {
    // Arrange
    let fixture = sql_fixture(
        "rename-include",
        &[
            "CREATE TABLE inc (id INT, k INT, extra TEXT)",
            "CREATE INDEX inc_ix ON inc (k) INCLUDE (extra)",
            "INSERT INTO inc VALUES (1, 1, 'e')",
            "ALTER TABLE inc RENAME COLUMN extra TO more",
            "INSERT INTO inc VALUES (2, 1, 'f')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT more FROM inc WHERE k = 1 ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::String("e".into())],
            vec![Value::String("f".into())]
        ]
    );
}

#[test]
fn should_accept_writes_after_renaming_column_of_distinct_from_check() {
    // Arrange
    let fixture = sql_fixture(
        "rename-distinct-check",
        &[
            "CREATE TABLE dchk (id INT, price INT CHECK (price IS DISTINCT FROM 0))",
            "ALTER TABLE dchk RENAME COLUMN price TO cost",
        ],
    );

    // Act
    let accepted = fixture.execute("INSERT INTO dchk VALUES (1, 5)");
    let rejected = fixture.execute("INSERT INTO dchk VALUES (2, 0)");

    // Assert
    assert!(accepted.is_ok(), "distinct value must be accepted");
    assert!(rejected.is_err(), "zero must still violate the check");
}

#[test]
fn should_accept_writes_after_renaming_column_of_not_distinct_from_check() {
    // Arrange
    let fixture = sql_fixture(
        "rename-not-distinct-check",
        &[
            "CREATE TABLE ndchk (id INT, price INT CHECK (price IS NOT DISTINCT FROM 7))",
            "ALTER TABLE ndchk RENAME COLUMN price TO cost",
        ],
    );

    // Act
    let accepted = fixture.execute("INSERT INTO ndchk VALUES (1, 7)");
    let rejected = fixture.execute("INSERT INTO ndchk VALUES (2, 8)");

    // Assert
    assert!(accepted.is_ok(), "matching value must be accepted");
    assert!(rejected.is_err(), "other value must violate the check");
}

#[test]
fn should_restore_index_predicate_when_column_rename_is_rolled_back() {
    // Arrange
    let fixture = sql_fixture(
        "rename-rollback-predicate",
        &[
            "CREATE TABLE rb (id INT, email TEXT, status TEXT)",
            "CREATE UNIQUE INDEX rb_ix ON rb (email) WHERE status = 'open'",
            "INSERT INTO rb VALUES (1, 'a@x', 'open')",
            "BEGIN",
            "ALTER TABLE rb RENAME COLUMN status TO state",
            "ROLLBACK",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO rb VALUES (2, 'a@x', 'open')");

    // Assert
    assert!(duplicate.is_err(), "rolled-back rename must keep the index");
}

#[test]
fn should_keep_partial_unique_index_enforced_after_table_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-table-partial-unique",
        &[
            "CREATE TABLE tr (id INT, email TEXT, status TEXT)",
            "CREATE UNIQUE INDEX tr_ix ON tr (email) WHERE status = 'open'",
            "INSERT INTO tr VALUES (1, 'a@x', 'open')",
            "ALTER TABLE tr RENAME TO tr2",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO tr2 VALUES (2, 'a@x', 'open')");

    // Assert
    assert!(duplicate.is_err(), "renamed table must keep the index");
}

#[test]
fn should_follow_rename_in_time_series_partition_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-time-series-partition",
        &[
            "CREATE TABLE ts (event_at TIMESTAMP, tenant TEXT, amount INT)",
            "CREATE INDEX ts_ix ON ts USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
            "INSERT INTO ts VALUES ('2026-01-01 00:00:00', 'a', 1)",
            "ALTER TABLE ts RENAME COLUMN tenant TO org",
            "INSERT INTO ts VALUES ('2026-01-01 00:30:00', 'a', 2)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT amount FROM ts WHERE org = 'a' ORDER BY amount");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_keep_non_unique_partial_index_complete_after_predicate_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "rename-partial-plain",
        &[
            "CREATE TABLE pp (id INT, k INT, status TEXT)",
            "CREATE INDEX pp_ix ON pp (k) WHERE status = 'open'",
            "INSERT INTO pp VALUES (1, 1, 'open')",
            "ALTER TABLE pp RENAME COLUMN status TO state",
            "INSERT INTO pp VALUES (2, 1, 'open')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT id FROM pp WHERE k = 1 AND state = 'open' ORDER BY id");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_keep_foreign_key_enforced_after_renaming_referencing_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-fk-child",
        &[
            "CREATE TABLE parent (id INT PRIMARY KEY)",
            "CREATE TABLE child (id INT, pid INT, FOREIGN KEY (pid) REFERENCES parent (id))",
            "INSERT INTO parent VALUES (1)",
            "ALTER TABLE child RENAME COLUMN pid TO parent_id",
        ],
    );

    // Act
    let valid = fixture.execute("INSERT INTO child VALUES (1, 1)");
    let orphan = fixture.execute("INSERT INTO child VALUES (2, 99)");

    // Assert
    assert!(valid.is_ok(), "existing parent must be accepted");
    assert!(orphan.is_err(), "missing parent must be rejected");
}

#[test]
fn should_keep_foreign_key_enforced_after_renaming_referenced_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-fk-parent",
        &[
            "CREATE TABLE parent2 (id INT PRIMARY KEY)",
            "CREATE TABLE child2 (id INT, pid INT, FOREIGN KEY (pid) REFERENCES parent2 (id))",
            "INSERT INTO parent2 VALUES (1)",
            "ALTER TABLE parent2 RENAME COLUMN id TO key",
        ],
    );

    // Act
    let valid = fixture.execute("INSERT INTO child2 VALUES (1, 1)");
    let orphan = fixture.execute("INSERT INTO child2 VALUES (2, 99)");

    // Assert
    assert!(valid.is_ok(), "existing parent must be accepted");
    assert!(orphan.is_err(), "missing parent must be rejected");
}

#[test]
fn should_keep_composite_unique_enforced_after_renaming_one_member_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-composite-unique",
        &[
            "CREATE TABLE cu (a INT, b INT, UNIQUE (a, b))",
            "INSERT INTO cu VALUES (1, 1)",
            "ALTER TABLE cu RENAME COLUMN b TO c",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO cu VALUES (1, 1)");
    let distinct = fixture.execute("INSERT INTO cu VALUES (1, 2)");

    // Assert
    assert!(duplicate.is_err(), "duplicate pair must be rejected");
    assert!(distinct.is_ok(), "distinct pair must be accepted");
}

#[test]
fn should_keep_primary_key_enforced_after_renaming_its_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-pk",
        &[
            "CREATE TABLE pk (id INT PRIMARY KEY, v INT)",
            "INSERT INTO pk VALUES (1, 1)",
            "ALTER TABLE pk RENAME COLUMN id TO key",
        ],
    );

    // Act
    let duplicate = fixture.execute("INSERT INTO pk VALUES (1, 2)");

    // Assert
    assert!(duplicate.is_err(), "duplicate key must be rejected");
}

#[test]
fn should_keep_identity_default_after_renaming_identity_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-identity",
        &[
            "CREATE TABLE ident (id INT GENERATED ALWAYS AS IDENTITY, v INT)",
            "INSERT INTO ident (v) VALUES (1)",
            "ALTER TABLE ident RENAME COLUMN id TO key",
            "INSERT INTO ident (v) VALUES (2)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT key FROM ident ORDER BY key");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_keep_unique_constraint_include_payload_after_renaming_included_column() {
    // Arrange
    let fixture = sql_fixture(
        "rename-unique-include",
        &[
            "CREATE TABLE ui (k INT, extra TEXT, UNIQUE (k) INCLUDE (extra))",
            "INSERT INTO ui VALUES (1, 'e')",
            "ALTER TABLE ui RENAME COLUMN extra TO more",
            "INSERT INTO ui VALUES (2, 'f')",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT more FROM ui WHERE k = 2");
    let duplicate = fixture.execute("INSERT INTO ui VALUES (1, 'g')");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("f".into())]]);
    assert!(duplicate.is_err(), "duplicate key must be rejected");
}
