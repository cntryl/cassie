//! PostgreSQL 18 large-object SQL and transactional behavior tracked by #916.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

#[test]
fn should_create_large_object_from_bytea_with_catalog_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-916-large-object-create", &[]);

    // Act
    let created = fixture
        .execute("SELECT lo_from_bytea(0, decode('0001ff', 'hex'))::text")
        .expect("create a large object from bytea");
    let object_id = created.rows()[0][0]
        .as_str()
        .expect("large-object OID text")
        .to_owned();
    let metadata = fixture
        .execute(&format!(
            "SELECT EXISTS (SELECT FROM pg_catalog.pg_largeobject_metadata \
             WHERE oid = {object_id}::oid)"
        ))
        .expect("inspect the created object's metadata");
    let bytes = fixture
        .execute(&format!("SELECT encode(lo_get({object_id}::oid), 'hex')"))
        .expect("read the newly created object's contents");

    // Assert
    assert!(object_id.parse::<u32>().expect("valid OID") > 0);
    assert_eq!(metadata.rows(), &[vec![Value::Bool(true)]]);
    assert_eq!(bytes.rows(), &[vec![Value::String("0001ff".to_owned())]]);
}

#[test]
fn should_return_zero_filled_bytes_for_sparse_large_object_reads() {
    // Arrange
    let fixture = sql_fixture("issue-916-large-object-sparse-read", &[]);
    let object_id = create_large_object(&fixture, "6162");

    // Act
    fixture
        .execute(&format!(
            "SELECT lo_put({object_id}::oid, 5::bigint, decode('ff', 'hex'))"
        ))
        .expect("write beyond the current object end");
    let sparse = fixture
        .execute(&format!(
            "SELECT encode(lo_get({object_id}::oid), 'hex'), \
                    encode(lo_get({object_id}::oid, 1::bigint, 3), 'hex')"
        ))
        .expect("read all sparse bytes and a bounded slice");

    // Assert
    assert_eq!(
        sparse.rows(),
        &[vec![
            Value::String("6162000000ff".to_owned()),
            Value::String("620000".to_owned()),
        ]]
    );
}

#[test]
fn should_rollback_large_object_byte_writes_to_savepoints() {
    // Arrange
    let fixture = sql_fixture("issue-916-large-object-savepoint", &[]);
    let object_id = create_large_object(&fixture, "616263");

    // Act
    fixture.execute("BEGIN").expect("begin savepoint test");
    fixture
        .execute("SAVEPOINT large_object_write")
        .expect("create savepoint");
    fixture
        .execute(&format!(
            "SELECT lo_put({object_id}::oid, 1::bigint, decode('ee', 'hex'))"
        ))
        .expect("write within the savepoint");
    fixture
        .execute("ROLLBACK TO SAVEPOINT large_object_write")
        .expect("roll back the large-object write");
    fixture
        .execute("RELEASE SAVEPOINT large_object_write")
        .expect("release the rolled-back savepoint");
    fixture.execute("COMMIT").expect("commit outer transaction");
    let after_rollback = fixture
        .execute(&format!("SELECT encode(lo_get({object_id}::oid), 'hex')"))
        .expect("read the object after savepoint rollback");

    // Assert
    assert_eq!(
        after_rollback.rows(),
        &[vec![Value::String("616263".to_owned())]]
    );
}

#[test]
fn should_remove_large_object_metadata_after_unlink() {
    // Arrange
    let fixture = sql_fixture("issue-916-large-object-unlink", &[]);
    let object_id = create_large_object(&fixture, "00ff");

    // Act
    let unlinked = fixture
        .execute(&format!("SELECT lo_unlink({object_id}::oid)::text"))
        .expect("unlink the large object");
    let missing = fixture
        .execute(&format!(
            "SELECT count(*)::text FROM pg_catalog.pg_largeobject_metadata \
             WHERE oid = {object_id}::oid"
        ))
        .expect("inspect the metadata catalog after unlink");

    // Assert
    assert_eq!(unlinked.rows(), &[vec![Value::String("1".to_owned())]]);
    assert_eq!(missing.rows(), &[vec![Value::String("0".to_owned())]]);
}

#[test]
fn should_preserve_large_object_64_bit_descriptor_contract() {
    // Arrange
    let fixture = sql_fixture("issue-916-large-object-descriptor-io", &[]);
    let object_id = create_large_object(&fixture, "61626364");
    fixture
        .execute("BEGIN")
        .expect("begin descriptor IO transaction");
    let descriptor = fixture
        .execute(&format!("SELECT lo_open({object_id}::oid, 393216)::text"))
        .expect("open the large object for reading and writing");
    let descriptor = descriptor.rows()[0][0]
        .as_str()
        .expect("large-object descriptor text")
        .to_owned();

    // Act
    let read = fixture
        .execute(&format!(
            "SELECT encode(loread({descriptor}::int4, 2), 'hex')"
        ))
        .expect("read two bytes from the descriptor");
    let position_after_read = fixture
        .execute(&format!("SELECT lo_tell64({descriptor}::int4)::text"))
        .expect("read the 64-bit descriptor position after reading");
    let written = fixture
        .execute(&format!(
            "SELECT lowrite({descriptor}::int4, decode('ff', 'hex'))::text"
        ))
        .expect("write one byte at the current descriptor position");
    let after_write = fixture
        .execute(&format!("SELECT encode(lo_get({object_id}::oid), 'hex')"))
        .expect("read the changed object bytes");
    let end_position = fixture
        .execute(&format!(
            "SELECT lo_lseek64({descriptor}::int4, 0::bigint, 2)::text"
        ))
        .expect("seek to the end using the 64-bit function");
    let high_position = fixture
        .execute(&format!(
            "SELECT lo_lseek64({descriptor}::int4, 2147483648::bigint, 0)::text"
        ))
        .expect("seek beyond the int32 range without allocating object bytes");
    let reported_high_position = fixture
        .execute(&format!("SELECT lo_tell64({descriptor}::int4)::text"))
        .expect("read the high 64-bit descriptor position");
    fixture
        .execute(&format!("SELECT lo_close({descriptor}::int4)::text"))
        .expect("close the large-object descriptor");
    fixture
        .execute("COMMIT")
        .expect("commit descriptor IO transaction");

    // Assert
    assert_eq!(read.rows(), &[vec![Value::String("6162".to_owned())]]);
    assert_eq!(
        position_after_read.rows(),
        &[vec![Value::String("2".to_owned())]]
    );
    assert_eq!(written.rows(), &[vec![Value::String("1".to_owned())]]);
    assert_eq!(
        after_write.rows(),
        &[vec![Value::String("6162ff64".to_owned())]]
    );
    assert_eq!(end_position.rows(), &[vec![Value::String("4".to_owned())]]);
    assert_eq!(
        high_position.rows(),
        &[vec![Value::String("2147483648".to_owned())]]
    );
    assert_eq!(
        reported_high_position.rows(),
        &[vec![Value::String("2147483648".to_owned())]]
    );
}

fn create_large_object(fixture: &SqlFixture, bytes: &str) -> String {
    let created = fixture
        .execute(&format!(
            "SELECT lo_from_bytea(0, decode('{bytes}', 'hex'))::text"
        ))
        .expect("create a large object from its initial bytes");
    created.rows()[0][0]
        .as_str()
        .expect("large-object OID text")
        .to_owned()
}
