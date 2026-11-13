//! Allocator-backed user routine identities across redefinition, drop and
//! restart (#818).

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::Cassie;
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 1\"",
    "CREATE FUNCTION routine_two(x INT) RETURNS INT AS \"x + 2\"",
    "CREATE PROCEDURE routine_three(note TEXT) AS \"SELECT 1\"",
];

struct RoutineDirectory(String);

impl Drop for RoutineDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn oid_of(fixture: &SqlFixture, signature: &str) -> i64 {
    match fixture
        .rows(&format!(
            "SELECT to_regprocedure('{signature}')::oid::bigint"
        ))
        .remove(0)
        .remove(0)
    {
        Value::Int64(oid) => oid,
        _ => panic!("expected an oid for {signature}"),
    }
}

#[test]
fn should_allocate_user_routine_oids_in_the_normal_user_range() {
    // Arrange
    let fixture = sql_fixture("routine_oid_range", SETUP);

    // Act
    let oids = [
        oid_of(&fixture, "routine_one(integer)"),
        oid_of(&fixture, "routine_two(integer)"),
        oid_of(&fixture, "routine_three(text)"),
    ];

    // Assert
    assert!(oids.iter().all(|oid| (16_384..1_i64 << 31).contains(oid)));
}

#[test]
fn should_allocate_distinct_oids_to_different_routines() {
    // Arrange
    let fixture = sql_fixture("routine_oid_distinct", SETUP);

    // Act
    let mut oids = vec![
        oid_of(&fixture, "routine_one(integer)"),
        oid_of(&fixture, "routine_two(integer)"),
        oid_of(&fixture, "routine_three(text)"),
    ];
    oids.sort_unstable();
    oids.dedup();

    // Assert
    assert_eq!(oids.len(), 3);
}

#[test]
fn should_allocate_a_new_oid_when_a_function_is_dropped_and_recreated() {
    // Arrange
    let fixture = sql_fixture("routine_oid_recreate", SETUP);
    let before = oid_of(&fixture, "routine_one(integer)");

    // Act
    fixture
        .execute("DROP FUNCTION routine_one")
        .expect("drop the function");
    fixture
        .execute("CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 1\"")
        .expect("recreate the function");
    let after = oid_of(&fixture, "routine_one(integer)");

    // Assert
    assert_ne!(before, after);
}

#[test]
fn should_keep_the_oid_when_a_function_is_redefined() {
    // Arrange
    let fixture = sql_fixture("routine_oid_redefine", SETUP);
    let before = oid_of(&fixture, "routine_one(integer)");

    // Act
    fixture
        .execute("CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 100\"")
        .expect("redefine the function");
    let after = oid_of(&fixture, "routine_one(integer)");

    // Assert
    assert_eq!(before, after);
}

#[test]
fn should_round_trip_a_routine_oid_through_its_regprocedure_text() {
    // Arrange
    let fixture = sql_fixture("routine_oid_round_trip", SETUP);

    // Act
    let rows = fixture.rows(
        "SELECT to_regprocedure('routine_two(integer)')::oid::regprocedure::oid \
                = to_regprocedure('routine_two(integer)')::oid",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_keep_a_function_oid_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("routine-oid-restart");
    let _directory = RoutineDirectory(path.clone());
    let query = "SELECT to_regprocedure('routine_one(integer)')::oid::bigint";
    let before = {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION routine_one(x INT) RETURNS INT AS \"x + 1\"",
                vec![],
            )
            .expect("create the function");
        cassie
            .execute_sql(&session, query, vec![])
            .expect("resolve before restart")
            .rows()
            .to_vec()
    };

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let after = cassie
        .execute_sql(&session, query, vec![])
        .expect("resolve after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(before, after);
}
