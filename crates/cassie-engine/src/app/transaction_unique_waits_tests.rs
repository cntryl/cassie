use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::sync::{mpsc, Arc};
use std::time::Instant;

fn unique_key() -> UniqueTransactionKey {
    UniqueTransactionKey {
        relation: "db.public.items".to_owned(),
        constraint: "items_label_key".to_owned(),
        field: "label".to_owned(),
        value: "\"shared\"".to_owned(),
    }
}

fn waiting_pair() -> (TransactionUniqueWaits, Uuid, Uuid, UniqueTransactionKey) {
    let waits = TransactionUniqueWaits::default();
    let owner = Uuid::new_v4();
    let waiter = Uuid::new_v4();
    let key = unique_key();
    waits
        .claim_keys(owner, &BTreeSet::from([key.clone()]))
        .expect("owner claims the key");
    let claim = waits
        .claim_keys(waiter, &BTreeSet::from([key.clone()]))
        .expect("waiter requests the held key");
    assert!(matches!(claim, UniqueKeyClaim::Wait { .. }));
    (waits, owner, waiter, key)
}

#[test]
fn should_clear_wait_edge_when_query_is_canceled() {
    // Arrange
    let (waits, owner, waiter, key) = waiting_pair();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );

    // Act
    let wait_id = waits
        .state
        .lock()
        .waits_for
        .get(&waiter)
        .expect("wait edge")
        .id;
    let result = waits.wait_for_key(waiter, owner, &key, wait_id, &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(!waits.state.lock().waits_for.contains_key(&waiter));
}

#[test]
fn should_clear_wait_edge_when_query_deadline_has_elapsed() {
    // Arrange
    let (waits, owner, waiter, key) = waiting_pair();
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 1,
        ..CassieRuntimeLimits::default()
    };
    let elapsed_start = Instant::now()
        .checked_sub(Duration::from_millis(10))
        .expect("subtract a short duration from the current instant");
    let controls = QueryExecutionControls::from_limits(&limits, elapsed_start);

    // Act
    let wait_id = waits
        .state
        .lock()
        .waits_for
        .get(&waiter)
        .expect("wait edge")
        .id;
    let result = waits.wait_for_key(waiter, owner, &key, wait_id, &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert!(!waits.state.lock().waits_for.contains_key(&waiter));
}

#[test]
fn should_not_clear_newer_wait_from_synchronized_obsolete_cleanup() {
    // Arrange
    let waits = Arc::new(TransactionUniqueWaits::default());
    let blocker = Uuid::new_v4();
    let waiter = Uuid::new_v4();
    let key = unique_key();
    waits
        .claim_keys(blocker, &BTreeSet::from([key.clone()]))
        .expect("blocker claims the key");
    let UniqueKeyClaim::Wait {
        wait_id: retired_wait_id,
        ..
    } = waits
        .claim_keys(waiter, &BTreeSet::from([key.clone()]))
        .expect("retired wait claim")
    else {
        panic!("expected the original wait");
    };
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let (waiting_sender, waiting_receiver) = mpsc::channel();
    let worker = {
        let waits = Arc::clone(&waits);
        let key = key.clone();
        std::thread::spawn(move || {
            let _observer = observe_next_unique_transaction_wait_for_test(move || {
                waiting_sender.send(()).expect("replacement observer");
            });
            waits.wait_for_key(waiter, blocker, &key, retired_wait_id, &controls)
        })
    };
    waiting_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("obsolete waiter reaches the condition variable");
    let newer_wait_id = Uuid::new_v4();

    // Act
    {
        let mut state = waits.state.lock();
        assert_eq!(
            state.waits_for.insert(
                waiter,
                WaitEdge {
                    blocker,
                    id: newer_wait_id,
                },
            ),
            Some(WaitEdge {
                blocker,
                id: retired_wait_id,
            })
        );
        cancellation.cancel();
        waits.changed.notify_all();
    }
    let result = worker.join().expect("obsolete cleanup worker");

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(
        waits
            .state
            .lock()
            .waits_for
            .get(&waiter)
            .map(|edge| edge.id),
        Some(newer_wait_id)
    );
}

#[test]
fn should_reject_a_unique_key_wait_cycle() {
    // Arrange
    let waits = TransactionUniqueWaits::default();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let first_key = UniqueTransactionKey {
        value: "\"first\"".to_owned(),
        ..unique_key()
    };
    let second_key = UniqueTransactionKey {
        value: "\"second\"".to_owned(),
        ..unique_key()
    };

    // Act
    let first_claim = waits
        .claim_keys(first, &BTreeSet::from([first_key.clone()]))
        .expect("first key claim");
    let second_claim = waits
        .claim_keys(second, &BTreeSet::from([second_key.clone()]))
        .expect("second key claim");
    let first_wait = waits
        .claim_keys(first, &BTreeSet::from([second_key]))
        .expect("first wait");
    let cycle = waits.claim_keys(second, &BTreeSet::from([first_key]));

    // Assert
    assert_eq!(first_claim, UniqueKeyClaim::Claimed);
    assert_eq!(second_claim, UniqueKeyClaim::Claimed);
    assert!(matches!(first_wait, UniqueKeyClaim::Wait { .. }));
    assert!(matches!(
        cycle,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::DeadlockDetected,
            ..
        })
    ));
}

#[test]
fn should_release_row_owner_for_a_later_transaction() {
    // Arrange
    let waits = TransactionUniqueWaits::default();
    let owner = Uuid::new_v4();
    let waiter = Uuid::new_v4();
    let ids = BTreeSet::from(["row-1".to_owned()]);
    assert_eq!(
        waits
            .claim_row_keys(owner, "items", &ids)
            .expect("row claim"),
        RowKeyClaim::Claimed
    );

    // Act
    waits.release_owner(owner);
    let claim = waits
        .claim_row_keys(waiter, "items", &ids)
        .expect("released row claim");

    // Assert
    assert_eq!(claim, RowKeyClaim::Claimed);
}

#[test]
fn should_cancel_a_row_wait_without_retaining_wait_edge() {
    // Arrange
    let waits = TransactionUniqueWaits::default();
    let owner = Uuid::new_v4();
    let waiter = Uuid::new_v4();
    let ids = BTreeSet::from(["row-1".to_owned()]);
    waits
        .claim_row_keys(owner, "items", &ids)
        .expect("row claim");
    let claim = waits
        .claim_row_keys(waiter, "items", &ids)
        .expect("row wait");
    let RowKeyClaim::Wait {
        collection,
        id,
        owner: blocker,
        wait_id,
    } = claim
    else {
        panic!("expected row wait");
    };
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );

    // Act
    let result = waits.wait_for_row(waiter, blocker, &collection, &id, wait_id, &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(!waits.state.lock().waits_for.contains_key(&waiter));
}
