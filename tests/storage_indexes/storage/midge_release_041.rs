//! Fresh-store admission and SQL recovery through the public Cassie API.
use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;

struct Store(PathBuf);
impl Store {
    fn new(label: &str) -> Self {
        crate::support_sql::use_local_storage();
        let path = PathBuf::from(crate::support_sql::data_dir(label));
        std::fs::create_dir_all(&path).expect("fresh fixture directory");
        Self(path)
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            assert!(std::thread::panicking(), "fixture cleanup: {error}");
        }
    }
}

#[test]
fn should_create_format_eight_given_a_fresh_store_when_cassie_starts() {
    // Arrange
    let store = Store::new("midge041-fresh");
    // Act
    let cassie = Cassie::new_with_data_dir(&store.0).expect("fresh Midge store");
    cassie.startup().expect("startup");
    let marker = std::fs::read(store.0.join("FORMAT")).expect("format marker");
    // Assert
    assert_eq!(marker, b"midge-format-version=8\n");
}

#[test]
fn should_reject_old_format_given_a_preserved_marker_when_cassie_opens() {
    // Arrange
    let store = Store::new("midge041-old-format");
    let marker = b"midge-format-version=4\n";
    std::fs::write(store.0.join("FORMAT"), marker).expect("old format fixture");
    std::fs::write(store.0.join("operator-backup"), b"preserve").expect("sentinel");
    // Act
    let result = Cassie::new_with_data_dir(&store.0);
    // Assert
    let error = result
        .err()
        .expect("pre-0.4 stores require logical migration");
    assert!(error.to_string().contains("format"), "{error}");
    assert_eq!(
        std::fs::read(store.0.join("FORMAT")).expect("marker"),
        marker
    );
    assert_eq!(
        std::fs::read(store.0.join("operator-backup")).expect("sentinel"),
        b"preserve"
    );
}

#[test]
fn should_recover_sql_state_given_a_fresh_store_when_cassie_restarts() {
    // Arrange
    let store = Store::new("midge041-sql-restart");
    let sql = "SELECT id,body,amount FROM persisted_view ORDER BY id";
    let before = {
        let cassie = Cassie::new_with_data_dir(&store.0).expect("fresh store");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE persisted_rows(id INT,body TEXT,amount BIGINT)",
            "CREATE INDEX persisted_body ON persisted_rows(body)",
            "CREATE VIEW persisted_view AS SELECT id,body,amount FROM persisted_rows",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect("seed metadata");
        }
        cassie
            .execute_sql(
                &session,
                "INSERT INTO persisted_rows VALUES (1,'committed',$1)",
                vec![Value::Int64(i64::MAX)],
            )
            .expect("commit typed row");
        let result = cassie
            .execute_sql(&session, sql, vec![])
            .expect("before restart");
        cassie.shutdown();
        result
    };
    // Act
    let restarted = Cassie::new_with_data_dir(&store.0).expect("reopen FORMAT 8");
    restarted.startup().expect("rehydrate metadata");
    let session = restarted.create_session("tester", None);
    let after = restarted
        .execute_sql(&session, sql, vec![])
        .expect("recovered view");
    // Assert
    assert_eq!(before.rows(), after.rows());
    assert_eq!(
        after.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("committed".to_owned()),
            Value::Int64(i64::MAX)
        ]]
    );
    assert_eq!(
        before
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        after
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>()
    );
    let collection = crate::support_sql::canonical_test_collection(&restarted, "persisted_rows");
    assert!(restarted
        .catalog
        .list_indexes(&collection)
        .iter()
        .any(|index| index.name.ends_with("persisted_body")));
}

#[test]
fn should_persist_post_recovery_writes_given_a_fresh_store_when_cassie_restarts_again() {
    // Arrange
    let store = Store::new("midge041-recovery-write");
    {
        let cassie = Cassie::new_with_data_dir(&store.0).expect("fresh store");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE recovery_writes(id INT,body TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO recovery_writes VALUES (1,'before')",
                vec![],
            )
            .expect("initial commit");
        cassie.shutdown();
    }
    // Act
    {
        let cassie = Cassie::new_with_data_dir(&store.0).expect("first reopen");
        cassie.startup().expect("first restart");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "INSERT INTO recovery_writes VALUES (2,'after')",
                vec![],
            )
            .expect("post-recovery commit");
        cassie.shutdown();
    }
    let cassie = Cassie::new_with_data_dir(&store.0).expect("second reopen");
    cassie.startup().expect("second restart");
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id,body FROM recovery_writes ORDER BY id",
            vec![],
        )
        .expect("read both commits");
    // Assert
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::String("before".to_owned())],
            vec![Value::Int64(2), Value::String("after".to_owned())]
        ]
    );
}
