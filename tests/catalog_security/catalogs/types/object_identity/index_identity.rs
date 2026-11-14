//! Index relations have collision-free `pg_class.oid` identities that survive
//! rename and restart (#817, #818).

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::Cassie;
use cassie::types::Value;

/// `cx_3327` and `cx_5040` shared one OID under the former name hash.
const SETUP: &[&str] = &[
    "CREATE TABLE ct_b (id INT PRIMARY KEY, v INT)",
    "CREATE INDEX cx_3327 ON ct_b (id)",
    "CREATE INDEX cx_5040 ON ct_b (v)",
];

struct IndexDirectory(String);

impl Drop for IndexDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

#[test]
fn should_give_two_indexes_distinct_oids_whatever_their_names() {
    // Arrange
    let fixture = fixture("index_identity_distinct");

    // Act
    let rows = fixture.rows(
        "SELECT 'cx_3327'::regclass::oid <> 'cx_5040'::regclass::oid, \
                'cx_3327'::regclass::text, 'cx_5040'::regclass::text",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::String("cx_3327".to_string()),
            Value::String("cx_5040".to_string()),
        ]]
    );
}

#[test]
fn should_publish_no_duplicate_relation_oid_in_pg_class() {
    // Arrange
    let fixture = fixture("index_identity_pg_class_unique");

    // Act
    let rows = fixture.rows("SELECT count(*) = count(DISTINCT oid) FROM pg_class");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_keep_an_index_oid_when_the_index_is_renamed() {
    // Arrange
    let fixture = fixture("index_identity_rename");
    let before = fixture.rows("SELECT 'cx_5040'::regclass::oid::bigint");

    // Act
    fixture
        .execute("ALTER INDEX cx_5040 RENAME TO cx_renamed")
        .expect("rename the index");
    let after = fixture.rows("SELECT 'cx_renamed'::regclass::oid::bigint");

    // Assert
    assert_eq!(before, after);
}

#[test]
fn should_keep_an_index_oid_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("index-oid-restart");
    let _directory = IndexDirectory(path.clone());
    let query = "SELECT 'cx_3327'::regclass::oid::bigint, 'cx_5040'::regclass::oid::bigint";
    let before = {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        for sql in SETUP {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("create the indexed table");
        }
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
    assert_ne!(before[0][0], before[0][1]);
}
