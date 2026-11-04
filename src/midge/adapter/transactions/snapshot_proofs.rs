//! Executable capability probes against the frozen published Midge dependency.

use super::{AdapterTransaction, ExpectedStorageValue, Midge, TransactionMode};
use cntryl_midge::{ConflictPolicy, MidgeError};

fn memory_engine() -> Midge {
    let midge = Midge::new_in_memory("postgres").expect("memory Midge adapter");
    midge.ensure_families_ready().expect("bootstrap families");
    midge
}

fn writer(midge: &Midge) -> AdapterTransaction {
    midge
        .begin_raw_database_adapter_tx("postgres", TransactionMode::ReadWrite)
        .expect("direct published read-write transaction")
}

fn put(midge: &Midge, key: &[u8], value: &[u8]) {
    let mut transaction = writer(midge);
    transaction
        .put(key.to_vec(), value.to_vec(), None)
        .expect("put");
    transaction
        .commit(midge.write_options_sync())
        .expect("commit");
}

#[test]
fn should_reject_retained_row_assertion_after_aba() {
    // Arrange
    let midge = memory_engine();
    let row = b"rr-proof-row";
    put(&midge, row, b"original");
    let mut retained = writer(&midge);
    retained.set_conflict_policy(ConflictPolicy::LastWriteWins);
    retained
        .assert_expected_value(
            row.to_vec(),
            &ExpectedStorageValue::Present(b"original".to_vec()),
        )
        .expect("assert row at the retained start sequence");
    put(&midge, row, b"intermediate");
    put(&midge, row, b"original");
    retained
        .put(row.to_vec(), b"candidate".to_vec(), None)
        .expect("prepare retained writer");

    // Act
    let result = retained.commit(midge.write_options_sync());

    // Assert
    assert!(matches!(result, Err(MidgeError::WriteConflict(_))));
    let fresh = writer(&midge);
    assert_eq!(
        fresh.get(row).expect("fresh row").as_deref(),
        Some(&b"original"[..])
    );
}

#[test]
fn should_allow_disjoint_retained_row_assertion_with_shared_metadata_write() {
    // Arrange
    let midge = memory_engine();
    let row_a = b"rr-proof-row-a";
    let row_b = b"rr-proof-row-b";
    let shared = b"rr-proof-shared-metadata";
    for (key, value) in [
        (&row_a[..], &b"old-a"[..]),
        (&row_b[..], &b"old-b"[..]),
        (&shared[..], &[0][..]),
    ] {
        put(&midge, key, value);
    }
    let mut retained = writer(&midge);
    retained.set_conflict_policy(ConflictPolicy::LastWriteWins);
    retained
        .assert_expected_value(
            row_a.to_vec(),
            &ExpectedStorageValue::Present(b"old-a".to_vec()),
        )
        .expect("assert only the written authoritative row");
    let mut other = writer(&midge);
    other
        .put(row_b.to_vec(), b"new-b".to_vec(), None)
        .expect("disjoint row");
    other
        .put(shared.to_vec(), vec![1], None)
        .expect("shared metadata");
    other
        .commit(midge.write_options_sync())
        .expect("commit other writer");
    let current = writer(&midge);
    let latest = current
        .get(shared)
        .expect("fresh shared metadata")
        .expect("metadata exists");
    assert_eq!(latest.as_ref(), &[1]);
    drop(current);
    retained
        .put(row_a.to_vec(), b"new-a".to_vec(), None)
        .expect("own row");
    retained
        .put(shared.to_vec(), vec![latest[0] + 1], None)
        .expect("fresh derived metadata");

    // Act
    let result = retained.commit(midge.write_options_sync());

    // Assert
    result.expect("shared metadata is not a whole-write conflict fence");
    let fresh = writer(&midge);
    assert_eq!(
        fresh.get(row_a).expect("row a").as_deref(),
        Some(&b"new-a"[..])
    );
    assert_eq!(
        fresh.get(row_b).expect("row b").as_deref(),
        Some(&b"new-b"[..])
    );
    assert_eq!(
        fresh.get(shared).expect("shared").as_deref(),
        Some(&[2][..])
    );
}

#[test]
fn should_expose_distinct_native_views_when_a_writer_bypasses_collection_gates() {
    // Arrange
    let midge = memory_engine();
    let row = b"rr-proof-ungated-native-row";
    put(&midge, row, b"before");

    // Act
    let (older, newer) = midge.with_collection_gates(&["postgres.public.proof".to_owned()], || {
        let older = writer(&midge);
        // This published adapter API returns a native transaction. It does not
        // participate in the SQL collection or catalog-publication gates.
        let mut native = midge
            .database_tx("postgres", TransactionMode::ReadWrite)
            .expect("published native transaction");
        native
            .put(row.to_vec(), b"after".to_vec(), None)
            .expect("native put");
        native
            .commit(midge.write_options_sync())
            .expect("native commit");
        let newer = writer(&midge);
        (older, newer)
    });

    // Assert
    assert_eq!(
        older.get(row).expect("older").as_deref(),
        Some(&b"before"[..])
    );
    assert_eq!(
        newer.get(row).expect("newer").as_deref(),
        Some(&b"after"[..])
    );
}

#[test]
fn should_reject_a_newer_expected_value_against_an_older_assertion_snapshot() {
    // Arrange
    let midge = memory_engine();
    let row = b"rr-proof-different-starts";
    put(&midge, row, b"before");
    let mut older = writer(&midge);
    older.set_conflict_policy(ConflictPolicy::LastWriteWins);
    put(&midge, row, b"after");
    let newer = writer(&midge);
    let newer_value = newer.get(row).expect("newer snapshot").expect("row");
    older
        .assert_expected_value(
            row.to_vec(),
            &ExpectedStorageValue::Present(newer_value.to_vec()),
        )
        .expect("assertion admission does not equate the two snapshots");

    // Act
    let result = older.commit(midge.write_options_sync());

    // Assert
    assert!(matches!(result, Err(MidgeError::WriteConflict(_))));
}
