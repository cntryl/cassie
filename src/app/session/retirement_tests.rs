//! Transaction retirement completes under the session transaction mutex.

use super::CassieSession;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use std::cell::RefCell;

type Observer = Box<dyn FnOnce(&CassieSession)>;

thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}

/// Called by COMMIT and ROLLBACK after cleanup and before the mutex is released.
pub(super) fn observe_retirement(session: &CassieSession) {
    let observer = OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(observer) = observer {
        observer(session);
    }
}

fn observe_next_retirement(observer: impl FnOnce(&CassieSession) + 'static) {
    OBSERVER.with(|slot| {
        assert!(slot.borrow_mut().replace(Box::new(observer)).is_none());
    });
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("owner", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

/// What a competing BEGIN could observe at the end of retirement.
#[derive(Debug, PartialEq, Eq)]
struct Retired {
    mutex_held: bool,
    timeout: String,
}

fn observed_retirement(terminal: &str) -> (Retired, String) {
    let (cassie, session) = fixture();
    let original = session.setting("statement_timeout").unwrap();
    run(&cassie, &session, "BEGIN");
    run(&cassie, &session, "SET LOCAL statement_timeout = '123'");
    let (sender, receiver) = std::sync::mpsc::channel();
    observe_next_retirement(move |session| {
        sender
            .send(Retired {
                mutex_held: session.transaction.try_lock().is_none(),
                timeout: session.settings.lock().get("statement_timeout").unwrap(),
            })
            .unwrap();
    });
    run(&cassie, &session, terminal);
    (receiver.try_recv().unwrap(), original)
}

#[test]
fn should_finish_commit_cleanup_before_releasing_the_transaction_mutex() {
    // Arrange / Act
    let (retired, original) = observed_retirement("COMMIT");

    // Assert
    assert_eq!(
        retired,
        Retired {
            mutex_held: true,
            timeout: original
        }
    );
}

#[test]
fn should_finish_rollback_cleanup_before_releasing_the_transaction_mutex() {
    // Arrange / Act
    let (retired, original) = observed_retirement("ROLLBACK");

    // Assert
    assert_eq!(
        retired,
        Retired {
            mutex_held: true,
            timeout: original
        }
    );
}

#[test]
fn should_not_carry_set_local_into_the_next_transaction_after_commit() {
    // Arrange
    let (cassie, session) = fixture();
    let original = session.setting("statement_timeout").unwrap();
    run(&cassie, &session, "BEGIN");
    run(&cassie, &session, "SET LOCAL statement_timeout = '123'");
    run(&cassie, &session, "COMMIT");

    // Act
    run(&cassie, &session, "BEGIN");
    let inside = session.setting("statement_timeout").unwrap();
    run(&cassie, &session, "ROLLBACK");
    let after = session.setting("statement_timeout").unwrap();

    // Assert
    assert_eq!(inside, original);
    assert_eq!(after, original);
}

#[test]
fn should_start_a_new_transaction_from_a_clone_after_retirement() {
    // Arrange
    let (cassie, session) = fixture();
    let clone = session.clone();
    run(
        &cassie,
        &session,
        "CREATE TABLE retirement_rows (value INT)",
    );
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "INSERT INTO retirement_rows (value) VALUES (1)",
    );
    run(&cassie, &session, "ROLLBACK");

    // Act
    run(&cassie, &clone, "BEGIN");
    run(
        &cassie,
        &clone,
        "INSERT INTO retirement_rows (value) VALUES (2)",
    );
    run(&cassie, &clone, "COMMIT");
    let rows = cassie
        .execute_sql(&session, "SELECT value FROM retirement_rows", vec![])
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![crate::types::Value::Int64(2)]]);
}
