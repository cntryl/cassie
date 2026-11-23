use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;

type PublicationObserver = Box<dyn FnOnce()>;

thread_local! {
    static BEFORE_PUBLICATION: RefCell<Option<PublicationObserver>> = const { RefCell::new(None) };
}

pub(super) fn before_unique_publication() {
    if let Some(observer) = BEFORE_PUBLICATION.take() {
        observer();
    }
}

#[test]
fn should_not_restore_a_unique_claim_removed_by_savepoint_rollback() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("owner", None);
    for sql in [
        "CREATE TABLE savepoint_claims (id INT PRIMARY KEY, v INT UNIQUE)",
        "BEGIN",
        "SAVEPOINT before_claim",
        "INSERT INTO savepoint_claims VALUES (1, 10)",
    ] {
        cassie.execute_sql(&session, sql, Vec::new()).unwrap();
    }
    let owner = session.transaction_unique_owner_id();
    let (paused, pause) = mpsc::channel();
    let (resume, resumed) = mpsc::channel();
    let worker = {
        let cassie = cassie.clone();
        let session = session.clone();
        std::thread::spawn(move || {
            BEFORE_PUBLICATION.set(Some(Box::new(move || {
                paused.send(()).unwrap();
                resumed.recv_timeout(Duration::from_secs(5)).unwrap();
            })));
            cassie.execute_sql(
                &session,
                "INSERT INTO savepoint_claims VALUES (2, 20)",
                Vec::new(),
            )
        })
    };
    pause.recv_timeout(Duration::from_secs(5)).unwrap();

    // Act
    session.rollback_to_savepoint("before_claim").unwrap();
    resume.send(()).unwrap();
    let stale = worker.join().unwrap();

    // Assert
    assert!(stale.is_err());
    assert!(cassie
        .transaction_unique_waits
        .snapshot_owner_keys(owner)
        .is_empty());
    assert_eq!(session.transaction_status(), "in_transaction");
    cassie.execute_sql(&session, "ROLLBACK", Vec::new()).unwrap();
}
