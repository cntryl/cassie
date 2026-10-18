use std::path::{Path, PathBuf};

use cassie::app::Cassie;
use cassie::types::Value;
use uuid::Uuid;

struct TestDataDirectory(PathBuf);

impl TestDataDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-postgresql18-large-object-restart-{}",
            Uuid::new_v4()
        ));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_restore_large_object_payload_after_storage_restart() {
    // Arrange
    let directory = TestDataDirectory::new();
    let object_id = {
        let cassie = Cassie::new_with_data_dir(directory.path()).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        let created = cassie
            .execute_sql(
                &session,
                "SELECT lo_from_bytea(0, decode('7061796c6f6164', 'hex'))::BIGINT",
                vec![],
            )
            .expect("create a PostgreSQL large object with a known byte sequence")
            .rows()
            .to_vec();
        let object_id = created[0][0]
            .as_i64()
            .expect("lo_from_bytea returns the large-object OID");
        assert_ne!(object_id, 0);
        drop(session);
        drop(cassie);
        object_id
    };

    // Act
    let cassie = Cassie::new_with_data_dir(directory.path()).expect("reopen Cassie instance");
    cassie.startup().expect("recover the stored large object");
    let session = cassie.create_session("tester", None);
    let payload_matches = cassie
        .execute_sql(
            &session,
            &format!("SELECT lo_get({object_id}::oid) = decode('7061796c6f6164', 'hex')"),
            vec![],
        )
        .expect("read the large-object payload after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(payload_matches, vec![vec![Value::Bool(true)]]);
}
