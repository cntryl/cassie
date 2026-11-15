use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use super::CassieSession;

const HANG_GUARD: Duration = Duration::from_secs(5);
type OriginPause = (SyncSender<()>, Receiver<()>);

thread_local! {
    static AFTER_IMPLICIT_ORIGIN: RefCell<Option<OriginPause>> = const { RefCell::new(None) };
}

pub(super) fn pause_after_implicit_origin_publication() {
    let pause = AFTER_IMPLICIT_ORIGIN.with(|slot| slot.borrow_mut().take());
    if let Some((ready, resume)) = pause {
        ready.send(()).expect("report origin publication");
        resume
            .recv_timeout(HANG_GUARD)
            .expect("resume origin worker");
    }
}

#[test]
fn should_publish_implicit_origin_with_transaction_generation() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    let worker_session = session.clone();
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);
    let (resume_tx, resume_rx) = mpsc::sync_channel(0);
    let worker = std::thread::spawn(move || {
        AFTER_IMPLICIT_ORIGIN.with(|slot| {
            *slot.borrow_mut() = Some((ready_tx, resume_rx));
        });
        worker_session.begin_implicit_transaction()
    });

    // Act
    ready_rx.recv_timeout(HANG_GUARD).expect("origin boundary");
    let active = session.is_transaction_active();
    let implicit = session.is_implicit_transaction();
    let generation = session.transaction_generation();
    resume_tx.send(()).expect("release origin worker");
    worker.join().expect("join origin worker").expect("begin");

    // Assert
    assert!(active);
    assert!(
        implicit,
        "the published generation must already be implicit"
    );
    assert_eq!(session.transaction_generation(), generation);
}

#[test]
fn should_preserve_generation_when_promoting_implicit_transaction() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    session
        .begin_implicit_transaction()
        .expect("implicit begin");
    session.mark_transaction_query();
    let generation = session.transaction_generation();
    let characteristics = session.transaction_characteristics();

    // Act
    session
        .begin_transaction(None)
        .expect("promote to explicit");

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_implicit_transaction());
    assert_eq!(session.transaction_generation(), generation);
    assert_eq!(session.transaction_characteristics(), characteristics);
}

#[test]
fn should_begin_explicit_transaction_with_explicit_origin() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);

    // Act
    session.begin_transaction(None).expect("explicit begin");

    // Assert
    assert!(session.is_transaction_active());
    assert!(!session.is_implicit_transaction());
}

#[test]
fn should_preserve_implicit_origin_on_repeated_implicit_begin() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    session
        .begin_implicit_transaction()
        .expect("implicit begin");
    session.mark_transaction_query();
    let generation = session.transaction_generation();
    let characteristics = session.transaction_characteristics();

    // Act
    session
        .begin_implicit_transaction()
        .expect("repeated begin");

    // Assert
    assert!(session.is_transaction_active());
    assert!(session.is_implicit_transaction());
    assert_eq!(session.transaction_generation(), generation);
    assert_eq!(session.transaction_characteristics(), characteristics);
}

#[test]
fn should_reject_implicit_begin_during_explicit_transaction() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    session.begin_transaction(None).expect("explicit begin");
    let generation = session.transaction_generation();

    // Act
    let result = session.begin_implicit_transaction();

    // Assert
    assert!(result.is_err());
    assert!(session.is_transaction_active());
    assert!(!session.is_implicit_transaction());
    assert_eq!(session.transaction_generation(), generation);
}

#[test]
fn should_reject_implicit_begin_during_failed_transaction() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    session
        .begin_implicit_transaction()
        .expect("implicit begin");
    session.mark_transaction_failed();
    let generation = session.transaction_generation();

    // Act
    let result = session.begin_implicit_transaction();

    // Assert
    assert!(result.is_err());
    assert!(session.is_transaction_failed());
    assert!(session.is_implicit_transaction());
    assert_eq!(session.transaction_generation(), generation);
}
