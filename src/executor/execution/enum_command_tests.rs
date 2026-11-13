//! Enum DDL laws: lifecycle, errors, dependents and transaction visibility.
//! Written before the implementation.

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<String, CassieError> {
    cassie
        .execute_sql(session, sql, vec![])
        .map(|result| result.command().to_owned())
}

fn failure(cassie: &Cassie, session: &CassieSession, sql: &str) -> CassieError {
    run(cassie, session, sql).expect_err("statement should fail")
}

fn labels(cassie: &Cassie, name: &str) -> Vec<String> {
    cassie
        .catalog
        .get_enum_type(name)
        .expect("enum type")
        .members
        .iter()
        .map(|member| member.label.clone())
        .collect()
}

#[test]
fn should_create_an_enum_with_ordered_distinct_members() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let command = run(
        &cassie,
        &session,
        "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
    )
    .expect("create");

    // Assert
    let stored = cassie.catalog.get_enum_type("mood").expect("registered");
    let orders = stored
        .members
        .iter()
        .map(|member| member.sort_order)
        .collect::<Vec<_>>();
    let mut ids = stored
        .members
        .iter()
        .map(|member| member.member_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(command, "CREATE TYPE");
    assert_eq!(orders, vec![1.0, 2.0, 3.0]);
    assert_eq!(ids.len(), 3);
    assert_eq!(stored.revision, 1);
}

#[test]
fn should_create_an_enum_without_labels() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let result = run(&cassie, &session, "CREATE TYPE nothing AS ENUM ()");

    // Assert
    assert!(result.is_ok());
    assert_eq!(labels(&cassie, "nothing"), Vec::<String>::new());
}

#[test]
fn should_reject_duplicate_labels_in_create_with_23505() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let error = failure(&cassie, &session, "CREATE TYPE bad AS ENUM ('a', 'b', 'a')");

    // Assert
    assert_eq!(error.sql_state(), "23505");
    assert!(!cassie.catalog.enum_type_exists("bad"));
}

#[test]
fn should_reject_a_label_longer_than_63_bytes_with_42602() {
    // Arrange
    let (cassie, session) = fixture();
    let long = "x".repeat(64);
    let exact = "y".repeat(63);

    // Act
    let create = failure(
        &cassie,
        &session,
        &format!("CREATE TYPE long_label AS ENUM ('{long}')"),
    );
    let boundary = run(
        &cassie,
        &session,
        &format!("CREATE TYPE boundary_label AS ENUM ('{exact}')"),
    );

    // Assert
    assert_eq!(create.sql_state(), "42602");
    assert!(boundary.is_ok());
}

#[test]
fn should_reject_a_type_name_that_is_already_taken_with_42710() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE mood AS ENUM ('a')").expect("first");
    run(&cassie, &session, "CREATE TABLE tbl (id BIGINT)").expect("table");

    // Act
    let twice = failure(&cassie, &session, "CREATE TYPE mood AS ENUM ('b')");
    let over_table = failure(&cassie, &session, "CREATE TYPE tbl AS ENUM ('b')");

    // Assert
    assert_eq!(twice.sql_state(), "42710");
    assert_eq!(over_table.sql_state(), "42710");
}

#[test]
fn should_reject_creation_outside_a_writable_existing_schema() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let system = failure(&cassie, &session, "CREATE TYPE pg_catalog.t AS ENUM ('a')");
    let unknown = failure(&cassie, &session, "CREATE TYPE nosuch.t AS ENUM ('a')");

    // Assert
    assert_eq!(system.sql_state(), "42501");
    assert_eq!(unknown.sql_state(), "3F000");
}

#[test]
fn should_add_values_at_every_placement_with_midpoint_sort_orders() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a', 'b', 'c')").expect("create");

    // Act
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'd'").expect("append");
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'a2' AFTER 'a'").expect("after");
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'z' BEFORE 'a'").expect("before");

    // Assert
    let stored = cassie.catalog.get_enum_type("t").expect("type");
    let orders = stored
        .members
        .iter()
        .map(|member| member.sort_order)
        .collect::<Vec<_>>();
    assert_eq!(labels(&cassie, "t"), ["z", "a", "a2", "b", "c", "d"]);
    assert_eq!(orders, vec![0.0, 1.0, 1.5, 2.0, 3.0, 4.0]);
    assert_eq!(stored.revision, 4);
}

#[test]
fn should_keep_member_ids_across_value_alterations() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a', 'b')").expect("create");
    let before = cassie.catalog.get_enum_type("t").expect("type").members;

    // Act
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'c' BEFORE 'b'").expect("add");
    run(
        &cassie,
        &session,
        "ALTER TYPE t RENAME VALUE 'a' TO 'first'",
    )
    .expect("rename");

    // Assert
    let after = cassie.catalog.get_enum_type("t").expect("type").members;
    assert_eq!(after[0].member_id, before[0].member_id);
    assert_eq!(after[0].label, "first");
    assert_eq!(after[2].member_id, before[1].member_id);
    assert!(after[1].member_id != before[0].member_id && after[1].member_id != before[1].member_id);
}

#[test]
fn should_report_add_value_errors_with_postgresql_sqlstates() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a', 'b')").expect("create");

    // Act
    let duplicate = failure(&cassie, &session, "ALTER TYPE t ADD VALUE 'a'");
    let missing_neighbour = failure(&cassie, &session, "ALTER TYPE t ADD VALUE 'x' BEFORE 'zz'");
    let too_long = failure(
        &cassie,
        &session,
        &format!("ALTER TYPE t ADD VALUE '{}'", "q".repeat(64)),
    );
    let if_not_exists = run(
        &cassie,
        &session,
        "ALTER TYPE t ADD VALUE IF NOT EXISTS 'a'",
    );

    // Assert
    assert_eq!(duplicate.sql_state(), "42710");
    assert_eq!(missing_neighbour.sql_state(), "22023");
    assert_eq!(too_long.sql_state(), "42602");
    assert_eq!(if_not_exists.expect("skipped"), "ALTER TYPE");
    assert_eq!(labels(&cassie, "t"), ["a", "b"]);
}

#[test]
fn should_report_rename_value_errors_with_postgresql_sqlstates() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a', 'b')").expect("create");

    // Act
    let existing = failure(&cassie, &session, "ALTER TYPE t RENAME VALUE 'a' TO 'b'");
    let missing = failure(&cassie, &session, "ALTER TYPE t RENAME VALUE 'zz' TO 'c'");

    // Assert
    assert_eq!(existing.sql_state(), "42710");
    assert_eq!(missing.sql_state(), "22023");
}

#[test]
fn should_resolve_a_renamed_moved_enum_under_its_new_name() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a')").expect("create");
    run(&cassie, &session, "CREATE SCHEMA archive").expect("schema");
    let oid = cassie.catalog.get_enum_type("t").expect("type").type_oid();

    // Act
    run(&cassie, &session, "ALTER TYPE t RENAME TO t2").expect("rename");
    run(&cassie, &session, "ALTER TYPE t2 SET SCHEMA archive").expect("move");

    // Assert
    let moved = cassie
        .catalog
        .list_enum_types()
        .into_iter()
        .find(|metadata| metadata.local_name() == "t2")
        .expect("moved type");
    assert_eq!(moved.type_oid(), oid);
    assert_eq!(moved.schema_name(), "archive");
    assert!(cassie.catalog.get_enum_type_by_oid(oid).is_some());
}

#[test]
fn should_refresh_the_descriptor_embedded_in_columns_after_each_alteration() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a')").expect("create");
    run(
        &cassie,
        &session,
        "CREATE TABLE tbl (id BIGINT, kind t, kinds t[])",
    )
    .expect("table");

    // Act
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'b'").expect("add");
    run(&cassie, &session, "ALTER TYPE t RENAME VALUE 'a' TO 'z'").expect("rename");

    // Assert
    let schema = cassie.catalog.get_schema("tbl").expect("schema");
    let embedded = schema
        .fields
        .iter()
        .find(|field| field.name == "kind")
        .and_then(|field| match &field.data_type {
            crate::types::DataType::Enum(descriptor) => Some(descriptor.clone()),
            _ => None,
        })
        .expect("enum column");
    let element = schema
        .fields
        .iter()
        .find(|field| field.name == "kinds")
        .and_then(|field| match &field.data_type {
            crate::types::DataType::Array(inner) => match inner.as_ref() {
                crate::types::DataType::Enum(descriptor) => Some(descriptor.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("enum array column");
    assert_eq!(embedded.labels().collect::<Vec<_>>(), ["z", "b"]);
    assert_eq!(embedded.revision, 3);
    assert_eq!(element, embedded);
}

#[test]
fn should_allow_drop_only_without_dependent_columns() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE used AS ENUM ('a')").expect("create");
    run(&cassie, &session, "CREATE TYPE free AS ENUM ('a')").expect("create");
    run(
        &cassie,
        &session,
        "CREATE TABLE tbl (id BIGINT, kinds used[])",
    )
    .expect("table");

    // Act
    let restricted = failure(&cassie, &session, "DROP TYPE used");
    let cascaded = failure(&cassie, &session, "DROP TYPE used CASCADE");
    let dropped = run(&cassie, &session, "DROP TYPE free");
    let missing = run(&cassie, &session, "DROP TYPE IF EXISTS nope");

    // Assert
    assert_eq!(restricted.sql_state(), "2BP01");
    assert_eq!(cascaded.sql_state(), "0A000");
    assert_eq!(dropped.expect("drop"), "DROP TYPE");
    assert_eq!(missing.expect("if exists"), "DROP TYPE");
    assert!(cassie.catalog.enum_type_exists("used"));
    assert!(!cassie.catalog.enum_type_exists("free"));
}

#[test]
fn should_reject_enums_nested_in_other_user_types() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE mood AS ENUM ('a')").expect("create");

    // Act
    let composite = failure(&cassie, &session, "CREATE TYPE pair AS (m mood)");
    let domain = failure(&cassie, &session, "CREATE DOMAIN mood_d AS mood");

    // Assert
    assert_eq!(composite.sql_state(), "0A000");
    assert_eq!(domain.sql_state(), "0A000");
}

#[test]
fn should_reject_alter_type_add_value_on_a_composite_type() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE pair AS (a INT)").expect("create");

    // Act
    let error = failure(&cassie, &session, "ALTER TYPE pair ADD VALUE 'x'");

    // Assert
    assert_eq!(error.sql_state(), "42809");
}

#[test]
fn should_discard_an_enum_created_in_a_rolled_back_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "CREATE TYPE tx AS ENUM ('a')").expect("create");

    // Act
    run(&cassie, &session, "ROLLBACK").expect("rollback");

    // Assert
    assert!(!cassie.catalog.enum_type_exists("tx"));
}

#[test]
fn should_publish_an_enum_created_in_a_committed_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "CREATE TYPE tx AS ENUM ('a')").expect("create");
    run(&cassie, &session, "ALTER TYPE tx ADD VALUE 'b'").expect("same-transaction add");

    // Act
    run(&cassie, &session, "COMMIT").expect("commit");

    // Assert
    assert_eq!(labels(&cassie, "tx"), ["a", "b"]);
}

#[test]
fn should_mark_a_value_added_to_an_existing_type_uncommitted_until_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a')").expect("create");
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'b'").expect("add");
    let overlay = session.transaction_catalog_overlay().expect("overlay");
    let inside = overlay.get_enum_type("t").expect("overlay type");

    // Act
    run(&cassie, &session, "COMMIT").expect("commit");

    // Assert
    let published = cassie.catalog.get_enum_type("t").expect("published type");
    assert_eq!(
        inside
            .members
            .iter()
            .map(|member| member.uncommitted)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
    assert!(published.members.iter().all(|member| !member.uncommitted));
}

#[test]
fn should_not_mark_a_value_uncommitted_when_the_type_was_created_in_the_same_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a')").expect("create");

    // Act
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'b'").expect("add");

    // Assert
    let overlay = session.transaction_catalog_overlay().expect("overlay");
    let inside = overlay.get_enum_type("t").expect("overlay type");
    assert!(inside.members.iter().all(|member| !member.uncommitted));
}

#[test]
fn should_restore_the_earlier_descriptor_when_a_savepoint_rolls_back() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "CREATE TYPE t AS ENUM ('a')").expect("create");
    run(&cassie, &session, "BEGIN").expect("begin");
    run(&cassie, &session, "SAVEPOINT s1").expect("savepoint");
    run(&cassie, &session, "ALTER TYPE t ADD VALUE 'b'").expect("add");

    // Act
    run(&cassie, &session, "ROLLBACK TO SAVEPOINT s1").expect("rollback to");
    run(&cassie, &session, "COMMIT").expect("commit");

    // Assert
    assert_eq!(labels(&cassie, "t"), ["a"]);
}
