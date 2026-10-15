use cassie::app::Cassie;
use cassie::midge::adapter::set_unique_constraint_cleanup_failure_point;

use super::support_sql as support;

#[test]
fn should_preserve_other_field_rules_when_named_primary_key_is_dropped() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-primary-key");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_primary (code TEXT, CONSTRAINT code_pk PRIMARY KEY (code), CONSTRAINT code_check CHECK (code <> ''))",
            vec![],
        )
        .expect("create table");

    // Act
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE drop_primary DROP CONSTRAINT code_pk",
            vec![],
        )
        .expect("drop primary key");

    // Assert
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_primary (code) VALUES (NULL)",
            vec![],
        )
        .expect("primary key rule removed");
    let check_error = cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_primary (code) VALUES ('')",
            vec![],
        )
        .expect_err("check remains");
    assert!(check_error.to_string().contains("check constraint"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allow_duplicate_values_when_named_unique_constraint_is_dropped() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-unique");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_unique (email TEXT, CONSTRAINT email_key UNIQUE (email))",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_unique (email) VALUES ('same@example.com')",
            vec![],
        )
        .expect("insert first row");

    // Act
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE drop_unique DROP CONSTRAINT email_key",
            vec![],
        )
        .expect("drop unique constraint");

    // Assert
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_unique (email) VALUES ('same@example.com')",
            vec![],
        )
        .expect("duplicate accepted");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reuse_unique_value_after_constraint_removal() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop_readd_unique_reservation");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE drop_readd_unique (email TEXT, CONSTRAINT email_key UNIQUE (email))",
        "INSERT INTO drop_readd_unique (email) VALUES ('a@x')",
        "ALTER TABLE drop_readd_unique DROP CONSTRAINT email_key",
        "DELETE FROM drop_readd_unique",
        "ALTER TABLE drop_readd_unique ADD CONSTRAINT email_uq UNIQUE (email)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO drop_readd_unique (email) VALUES ('a@x')",
        vec![],
    );

    // Assert
    assert!(inserted.is_ok(), "dropped constraint reservation leaked");
    let rows = cassie
        .execute_sql(&session, "SELECT email FROM drop_readd_unique", vec![])
        .expect("read rows");
    assert_eq!(rows.rows().len(), 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_reservation_when_primary_key_remains_after_unique_drop() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop_unique_keep_primary_reservation");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE drop_unique_keep_primary (id TEXT, CONSTRAINT id_pk PRIMARY KEY (id), CONSTRAINT id_uq UNIQUE (id))",
            "INSERT INTO drop_unique_keep_primary (id) VALUES ('same')",
            "ALTER TABLE drop_unique_keep_primary DROP CONSTRAINT id_uq",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO drop_unique_keep_primary (id) VALUES ('same')",
        vec![],
    );

    // Assert
    assert!(duplicate.is_err(), "primary key reservation was removed");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_unique_reservation_cleanup_after_interrupted_constraint_drop() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop_unique_reservation_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE drop_unique_recovery (email TEXT, CONSTRAINT email_key UNIQUE (email))",
        "INSERT INTO drop_unique_recovery (email) VALUES ('a@x')",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    set_unique_constraint_cleanup_failure_point(true);

    // Act
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE drop_unique_recovery DROP CONSTRAINT email_key",
        vec![],
    );
    drop(cassie);
    let recovered = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    recovered.startup().expect("replay reservation cleanup");
    let session = recovered.create_session("tester", None);
    for sql in [
        "DELETE FROM drop_unique_recovery",
        "ALTER TABLE drop_unique_recovery ADD CONSTRAINT email_uq UNIQUE (email)",
    ] {
        recovered.execute_sql(&session, sql, vec![]).expect(sql);
    }
    let inserted = recovered.execute_sql(
        &session,
        "INSERT INTO drop_unique_recovery (email) VALUES ('a@x')",
        vec![],
    );

    // Assert
    assert!(interrupted.is_err(), "failure point did not interrupt drop");
    assert!(inserted.is_ok(), "recovered drop left a stale reservation");
    let rows = recovered
        .execute_sql(&session, "SELECT email FROM drop_unique_recovery", vec![])
        .expect("read rows");
    assert_eq!(rows.rows().len(), 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_remove_named_check_plus_foreign_key_constraints() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-check-foreign-key");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_parents (id TEXT PRIMARY KEY)",
            vec![],
        )
        .expect("create parents");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE drop_children (parent_id TEXT, score INT, CONSTRAINT score_check CHECK (score > 0), CONSTRAINT parent_fk FOREIGN KEY (parent_id) REFERENCES drop_parents(id))",
            vec![],
        )
        .expect("create children");

    // Act
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE drop_children DROP CONSTRAINT score_check",
            vec![],
        )
        .expect("drop check");
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE drop_children DROP CONSTRAINT parent_fk",
            vec![],
        )
        .expect("drop foreign key");

    // Assert
    cassie
        .execute_sql(
            &session,
            "INSERT INTO drop_children (parent_id, score) VALUES ('missing', -1)",
            vec![],
        )
        .expect("removed constraints no longer enforce writes");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_dropping_unique_constraint_with_live_foreign_key_dependency() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-live-dependency");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE dependency_parents (email TEXT, CONSTRAINT dependency_email_key UNIQUE (email))",
            vec![],
        )
        .expect("create parents");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE dependency_children (email TEXT REFERENCES dependency_parents(email))",
            vec![],
        )
        .expect("create children");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "ALTER TABLE dependency_parents DROP CONSTRAINT dependency_email_key",
            vec![],
        )
        .expect_err("dependency blocks drop");

    // Assert
    assert!(error.to_string().contains("depends on it"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_hydrate_dropped_constraint_state_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-constraint-restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE restart_drop_unique (email TEXT, CONSTRAINT restart_email_key UNIQUE (email))",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE restart_drop_unique DROP CONSTRAINT restart_email_key",
                vec![],
            )
            .expect("drop constraint");
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened cassie");
    cassie.startup().expect("restart");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "INSERT INTO restart_drop_unique (email) VALUES ('same@example.com'), ('same@example.com')",
            vec![],
        )
        .expect("duplicates accepted after restart");

    // Assert
    let rows = cassie
        .execute_sql(&session, "SELECT email FROM restart_drop_unique", vec![])
        .expect("read rows");
    assert_eq!(rows.rows().len(), 2);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_drop_constraint_if_exists_for_missing_name() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("drop-constraint-if-exists");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE drop_if_exists (id TEXT)", vec![])
        .expect("create table");

    // Act
    let result = cassie.execute_sql(
        &session,
        "ALTER TABLE drop_if_exists DROP CONSTRAINT IF EXISTS missing_key",
        vec![],
    );

    // Assert
    result.expect("missing constraint ignored");

    let _ = std::fs::remove_dir_all(path);
}
