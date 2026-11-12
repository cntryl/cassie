//! Cancellation inside REPEATABLE READ reports 57014 and keeps the retained view recoverable.

use crate::support_pgwire as wire;
use crate::support_pgwire_cancel_session::{ready_status, scalar, CancelSession, Frames};

const COUNT: &str = "SELECT count(*) FROM rr_cancel_rows";
const SLEEP: &str = "SELECT pg_sleep_for(INTERVAL '60 seconds')";

fn assert_clean(label: &str, frames: &Frames, status: u8) {
    assert_eq!(wire::error_code(frames), None, "{label}: unexpected error");
    assert_eq!(ready_status(frames), Some(status), "{label}: ready status");
}

async fn open_retained_view(session: &mut CancelSession) {
    for (sql, status) in [
        ("CREATE TABLE rr_cancel_rows (id INT PRIMARY KEY)", b'I'),
        ("INSERT INTO rr_cancel_rows VALUES (1)", b'I'),
        ("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE", b'T'),
    ] {
        assert_clean(sql, &session.owner(sql).await, status);
    }
    assert_eq!(
        scalar(&session.owner(COUNT).await).as_deref(),
        Some("1"),
        "the first query captures the retained view"
    );
}

async fn cancel_in_retained_view(session: &mut CancelSession) -> Frames {
    let (canceled, _) = session
        .cancel_at_sleep(vec![wire::simple_query_frame(SLEEP)], COUNT)
        .await;
    canceled
}

#[test]
fn should_recover_to_savepoint_with_the_retained_view_after_canceled_repeatable_read() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let evidence = runtime.block_on(async {
        let mut session = CancelSession::start().await;
        open_retained_view(&mut session).await;
        let savepoint = session.owner("SAVEPOINT before_sleep").await;
        let competing = session
            .observer("INSERT INTO rr_cancel_rows VALUES (2)")
            .await;
        let canceled = cancel_in_retained_view(&mut session).await;
        let ignored = session.owner(COUNT).await;
        let recovered = session.owner("ROLLBACK TO SAVEPOINT before_sleep").await;
        let retained = session.owner(COUNT).await;
        let committed = session.owner("COMMIT").await;
        let fresh = session.owner(COUNT).await;
        session.finish().await;
        (
            savepoint, competing, canceled, ignored, recovered, retained, committed, fresh,
        )
    });
    let (savepoint, competing, canceled, ignored, recovered, retained, committed, fresh) = evidence;

    // Assert
    assert_clean("savepoint", &savepoint, b'T');
    assert_clean("competing commit", &competing, b'I');
    assert_eq!(wire::error_code(&canceled).as_deref(), Some("57014"));
    assert_eq!(ready_status(&canceled), Some(b'E'));
    assert!(!canceled.iter().any(|(tag, _)| *tag == b'C'));
    assert_eq!(
        wire::error_code(&ignored).as_deref(),
        Some("25P02"),
        "a failed block ignores commands until recovery"
    );
    assert_clean("savepoint recovery", &recovered, b'T');
    assert_eq!(
        scalar(&retained).as_deref(),
        Some("1"),
        "cancellation and savepoint recovery keep the original retained view"
    );
    assert_clean("commit", &committed, b'I');
    assert_eq!(scalar(&fresh).as_deref(), Some("2"));
}

#[test]
fn should_release_the_retained_view_after_canceled_repeatable_read_rollback() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let evidence = runtime.block_on(async {
        let mut session = CancelSession::start().await;
        open_retained_view(&mut session).await;
        let competing = session
            .observer("INSERT INTO rr_cancel_rows VALUES (2)")
            .await;
        let canceled = cancel_in_retained_view(&mut session).await;
        let rolled_back = session.owner("ROLLBACK").await;
        let fresh = session.owner(COUNT).await;
        let reopened = session
            .owner("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
            .await;
        let new_view = session.owner(COUNT).await;
        let closed = session.owner("COMMIT").await;
        session.finish().await;
        (
            competing,
            canceled,
            rolled_back,
            fresh,
            reopened,
            new_view,
            closed,
        )
    });
    let (competing, canceled, rolled_back, fresh, reopened, new_view, closed) = evidence;

    // Assert
    assert_clean("competing commit", &competing, b'I');
    assert_eq!(wire::error_code(&canceled).as_deref(), Some("57014"));
    assert_eq!(ready_status(&canceled), Some(b'E'));
    assert_clean("rollback", &rolled_back, b'I');
    assert_eq!(scalar(&fresh).as_deref(), Some("2"));
    assert_clean("reopen", &reopened, b'T');
    assert_eq!(
        scalar(&new_view).as_deref(),
        Some("2"),
        "a new transaction captures a new view after the canceled one retired"
    );
    assert_clean("close", &closed, b'I');
}
