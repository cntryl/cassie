//! Views depend on the enums their definitions name: renames keep them
//! working and `DROP TYPE` is refused while one exists (#827).

use crate::support_enum_types::strings;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str, setup: &[&str]) -> SqlFixture {
    let mut statements = vec![
        "CREATE TYPE stage AS ENUM ('a', 'b', 'c')",
        "CREATE TABLE steps (id int PRIMARY KEY, e stage, note text)",
        "INSERT INTO steps VALUES (1, 'b', 'b'), (2, 'c', 'b'), (3, 'a', 'b')",
    ];
    statements.extend_from_slice(setup);
    sql_fixture(label, &statements)
}

#[test]
fn should_keep_a_view_comparing_a_label_working_after_rename_value() {
    // Arrange
    let fixture = fixture(
        "enum_view_rename_value",
        &["CREATE VIEW late AS SELECT id FROM steps WHERE e >= 'b' AND note = 'b'"],
    );

    // Act
    let renamed = fixture.execute("ALTER TYPE stage RENAME VALUE 'b' TO 'bee'");
    let ids = strings(&fixture, "SELECT id::text FROM late ORDER BY id");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(ids, vec!["1", "2"]);
}

#[test]
fn should_keep_a_view_casting_a_label_working_after_rename_value() {
    // Arrange
    let fixture = fixture(
        "enum_view_rename_value_cast",
        &[
            "CREATE VIEW exact AS SELECT id, 'c'::stage AS x FROM steps \
           WHERE e = CAST('b' AS stage)",
        ],
    );

    // Act
    let renamed = fixture.execute("ALTER TYPE stage RENAME VALUE 'b' TO 'bee'");
    let rows = strings(&fixture, "SELECT id::text || ':' || x::text FROM exact");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec!["1:c"]);
}

#[test]
fn should_keep_a_view_casting_to_the_type_working_after_rename_to() {
    // Arrange
    let fixture = fixture(
        "enum_view_rename_type",
        &[
            "CREATE VIEW exact AS SELECT id, 'c'::stage AS x FROM steps \
           WHERE e = CAST('b' AS stage)",
        ],
    );

    // Act
    let renamed = fixture.execute("ALTER TYPE stage RENAME TO phase");
    let rows = strings(&fixture, "SELECT id::text || ':' || x::text FROM exact");

    // Assert
    assert!(renamed.is_ok());
    assert_eq!(rows, vec!["1:c"]);
}

#[test]
fn should_refuse_to_drop_a_type_a_view_output_depends_on() {
    // Arrange
    let fixture = sql_fixture(
        "enum_view_drop_output",
        &[
            "CREATE TYPE flag AS ENUM ('p')",
            "CREATE VIEW flagged AS SELECT 'p'::flag AS x",
        ],
    );

    // Act
    let error = fixture.error("DROP TYPE flag");
    let rows = strings(&fixture, "SELECT x::text FROM flagged");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
    assert_eq!(rows, vec!["p"]);
}

#[test]
fn should_refuse_to_drop_a_type_a_view_only_casts_to() {
    // Arrange
    let fixture = sql_fixture(
        "enum_view_drop_internal_cast",
        &[
            "CREATE TYPE flag AS ENUM ('p')",
            "CREATE VIEW checked AS SELECT 1 AS x WHERE 'p'::flag = 'p'::flag",
        ],
    );

    // Act
    let error = fixture.error("DROP TYPE flag");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_refuse_to_drop_a_type_a_routine_argument_depends_on() {
    // Arrange
    let fixture = sql_fixture(
        "enum_routine_drop",
        &[
            "CREATE TYPE flag AS ENUM ('p')",
            "CREATE FUNCTION is_flag(x flag) RETURNS int AS \"1\"",
        ],
    );

    // Act
    let error = fixture.error("DROP TYPE flag");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}
