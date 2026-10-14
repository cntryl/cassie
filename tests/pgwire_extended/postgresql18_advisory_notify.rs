//! PostgreSQL 18.6 advisory-lock and LISTEN/NOTIFY wire semantics for #822.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

type Frames = Vec<(u8, Vec<u8>)>;

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

async fn connect_session(
    server: &wire::PgwireServer,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire session");
    let (mut reader, mut writer) = socket.into_split();
    wire::complete_startup(&mut reader, &mut writer).await;
    (reader, writer)
}

async fn simple_query(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) -> Frames {
    wire::write_frames(writer, vec![wire::simple_query_frame(sql)]).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

fn assert_scalar_result(frames: &Frames, expected_oid: i32, expected_value: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "unexpected wire error: {frames:?}"
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("scalar RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, expected_oid);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(expected_value.to_owned())]]
    );
    assert_eq!(frames.last().map(|(tag, _)| *tag), Some(b'Z'));
}

fn assert_command(frames: &Frames, expected_tag: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "unexpected wire error: {frames:?}"
    );
    let command = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            let end = payload
                .iter()
                .position(|byte| *byte == 0)
                .expect("CommandComplete tag terminator");
            std::str::from_utf8(&payload[..end])
                .expect("CommandComplete tag is UTF-8")
                .to_owned()
        })
        .expect("CommandComplete");
    assert_eq!(command, expected_tag);
    assert_eq!(frames.last().map(|(tag, _)| *tag), Some(b'Z'));
}

fn assert_sqlstate(frames: &Frames, expected_code: &str) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some(expected_code),
        "unexpected SQLSTATE or wire response: {frames:?}"
    );
    assert_eq!(frames.last().map(|(tag, _)| *tag), Some(b'Z'));
}

fn notification_fields(payload: &[u8]) -> (i32, String, String) {
    assert!(
        payload.len() >= 7,
        "NotificationResponse payload is too short"
    );
    let process_id = i32::from_be_bytes(
        payload[..4]
            .try_into()
            .expect("NotificationResponse backend PID"),
    );
    let fields = &payload[4..];
    let channel_end = fields
        .iter()
        .position(|byte| *byte == 0)
        .expect("NotificationResponse channel terminator");
    let channel = std::str::from_utf8(&fields[..channel_end])
        .expect("notification channel is UTF-8")
        .to_owned();
    let payload_start = channel_end + 1;
    let notification_end = fields[payload_start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|offset| payload_start + offset)
        .expect("NotificationResponse payload terminator");
    let notification = std::str::from_utf8(&fields[payload_start..notification_end])
        .expect("notification payload is UTF-8")
        .to_owned();
    assert_eq!(notification_end + 1, fields.len());
    (process_id, channel, notification)
}

#[test]
fn should_observe_advisory_lock_ownership_across_pgwire_sessions() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-advisory-lock-pgwire-sessions");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (
        owner_lock,
        contender_busy,
        owner_unlock,
        contender_acquire,
        contender_unlock,
        owner_recovery,
        contender_recovery,
    ) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start advisory-lock fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut owner_reader, mut owner_writer) = connect_session(&server).await;
        let (mut contender_reader, mut contender_writer) = connect_session(&server).await;

        let owner_lock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_lock(82201822, 61)",
        )
        .await;
        let contender_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 61)",
        )
        .await;
        let owner_unlock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_unlock(82201822, 61)",
        )
        .await;
        let contender_acquire = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 61)",
        )
        .await;
        let contender_unlock = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_advisory_unlock(82201822, 61)",
        )
        .await;
        let owner_recovery = simple_query(&mut owner_reader, &mut owner_writer, "SELECT 44").await;
        let contender_recovery =
            simple_query(&mut contender_reader, &mut contender_writer, "SELECT 45").await;

        drop((
            owner_reader,
            owner_writer,
            contender_reader,
            contender_writer,
        ));
        server.stop().await;
        (
            owner_lock,
            contender_busy,
            owner_unlock,
            contender_acquire,
            contender_unlock,
            owner_recovery,
            contender_recovery,
        )
    });
    drop(runtime);

    // Assert
    assert_scalar_result(&owner_lock, 2278, "");
    assert_scalar_result(&contender_busy, 16, "f");
    assert_scalar_result(&owner_unlock, 16, "t");
    assert_scalar_result(&contender_acquire, 16, "t");
    assert_scalar_result(&contender_unlock, 16, "t");
    assert_scalar_result(&owner_recovery, 23, "44");
    assert_scalar_result(&contender_recovery, 23, "45");
}

#[test]
fn should_release_session_locks_with_unlock_all_over_pgwire() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-advisory-unlock-all-pgwire");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (
        owner_begin,
        owner_one_key_lock,
        owner_two_key_lock,
        owner_xact_lock,
        contender_one_key_busy,
        contender_two_key_busy,
        contender_xact_busy,
        owner_unlock_all,
        contender_one_key_acquire,
        contender_two_key_acquire,
        contender_one_key_unlock,
        contender_two_key_unlock,
        contender_xact_still_busy,
        owner_rollback,
        contender_xact_acquire,
        recovery,
    ) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start advisory unlock-all fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut owner_reader, mut owner_writer) = connect_session(&server).await;
        let (mut contender_reader, mut contender_writer) = connect_session(&server).await;

        let owner_begin = simple_query(&mut owner_reader, &mut owner_writer, "BEGIN").await;
        let owner_one_key_lock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_lock(82201823)",
        )
        .await;
        let owner_two_key_lock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_lock(82201822, 62)",
        )
        .await;
        let owner_xact_lock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_xact_lock(82201824, 63)",
        )
        .await;
        let contender_one_key_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201823)",
        )
        .await;
        let contender_two_key_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 62)",
        )
        .await;
        let contender_xact_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_xact_lock(82201824, 63)",
        )
        .await;
        let owner_unlock_all = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_unlock_all()",
        )
        .await;
        let contender_one_key_acquire = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201823)",
        )
        .await;
        let contender_two_key_acquire = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 62)",
        )
        .await;
        let contender_one_key_unlock = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_advisory_unlock(82201823)",
        )
        .await;
        let contender_two_key_unlock = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_advisory_unlock(82201822, 62)",
        )
        .await;
        let contender_xact_still_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_xact_lock(82201824, 63)",
        )
        .await;
        let owner_rollback = simple_query(&mut owner_reader, &mut owner_writer, "ROLLBACK").await;
        let contender_xact_acquire = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_xact_lock(82201824, 63)",
        )
        .await;
        let recovery =
            simple_query(&mut contender_reader, &mut contender_writer, "SELECT 47").await;

        drop((
            owner_reader,
            owner_writer,
            contender_reader,
            contender_writer,
        ));
        server.stop().await;
        (
            owner_begin,
            owner_one_key_lock,
            owner_two_key_lock,
            owner_xact_lock,
            contender_one_key_busy,
            contender_two_key_busy,
            contender_xact_busy,
            owner_unlock_all,
            contender_one_key_acquire,
            contender_two_key_acquire,
            contender_one_key_unlock,
            contender_two_key_unlock,
            contender_xact_still_busy,
            owner_rollback,
            contender_xact_acquire,
            recovery,
        )
    });
    drop(runtime);

    // Assert
    assert_command(&owner_begin, "BEGIN");
    assert_scalar_result(&owner_one_key_lock, 2278, "");
    assert_scalar_result(&owner_two_key_lock, 2278, "");
    assert_scalar_result(&owner_xact_lock, 2278, "");
    assert_scalar_result(&contender_one_key_busy, 16, "f");
    assert_scalar_result(&contender_two_key_busy, 16, "f");
    assert_scalar_result(&contender_xact_busy, 16, "f");
    assert_scalar_result(&owner_unlock_all, 2278, "");
    assert_scalar_result(&contender_one_key_acquire, 16, "t");
    assert_scalar_result(&contender_two_key_acquire, 16, "t");
    assert_scalar_result(&contender_one_key_unlock, 16, "t");
    assert_scalar_result(&contender_two_key_unlock, 16, "t");
    assert_scalar_result(&contender_xact_still_busy, 16, "f");
    assert_command(&owner_rollback, "ROLLBACK");
    assert_scalar_result(&contender_xact_acquire, 16, "t");
    assert_scalar_result(&recovery, 23, "47");
}

#[test]
fn should_deliver_listen_notify_payload_only_after_commit_over_pgwire() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-listen-notify-transaction-pgwire");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (
        listen,
        begin_rollback,
        notify_rollback,
        before_rollback,
        rollback,
        after_rollback,
        begin_commit,
        notify_commit,
        notify_commit_duplicate,
        commit,
        after_commit,
        recovery,
    ) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start LISTEN/NOTIFY fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut listener_reader, mut listener_writer) = connect_session(&server).await;
        let (mut publisher_reader, mut publisher_writer) = connect_session(&server).await;
        let channel = "issue_822_wire_events";

        let listen = simple_query(
            &mut listener_reader,
            &mut listener_writer,
            &format!("LISTEN {channel}"),
        )
        .await;
        let begin_rollback =
            simple_query(&mut publisher_reader, &mut publisher_writer, "BEGIN").await;
        let notify_rollback = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'rolled-back payload'"),
        )
        .await;
        let before_rollback =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 41").await;
        let rollback = simple_query(&mut publisher_reader, &mut publisher_writer, "ROLLBACK").await;
        let after_rollback =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 42").await;
        let begin_commit =
            simple_query(&mut publisher_reader, &mut publisher_writer, "BEGIN").await;
        let notify_commit = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'committed payload'"),
        )
        .await;
        let notify_commit_duplicate = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'committed payload'"),
        )
        .await;
        let commit = simple_query(&mut publisher_reader, &mut publisher_writer, "COMMIT").await;
        let after_commit =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 43").await;
        let recovery = simple_query(&mut listener_reader, &mut listener_writer, "SELECT 44").await;

        drop((
            listener_reader,
            listener_writer,
            publisher_reader,
            publisher_writer,
        ));
        server.stop().await;
        (
            listen,
            begin_rollback,
            notify_rollback,
            before_rollback,
            rollback,
            after_rollback,
            begin_commit,
            notify_commit,
            notify_commit_duplicate,
            commit,
            after_commit,
            recovery,
        )
    });
    drop(runtime);

    // Assert
    assert_command(&listen, "LISTEN");
    assert_command(&begin_rollback, "BEGIN");
    assert_command(&notify_rollback, "NOTIFY");
    assert_scalar_result(&before_rollback, 23, "41");
    assert!(
        before_rollback.iter().all(|(tag, _)| *tag != b'A'),
        "an uncommitted notification must not reach the listener"
    );
    assert_command(&rollback, "ROLLBACK");
    assert_scalar_result(&after_rollback, 23, "42");
    assert!(
        after_rollback.iter().all(|(tag, _)| *tag != b'A'),
        "a rolled-back notification must not reach the listener"
    );
    assert_command(&begin_commit, "BEGIN");
    assert_command(&notify_commit, "NOTIFY");
    assert_command(&notify_commit_duplicate, "NOTIFY");
    assert_command(&commit, "COMMIT");
    assert_scalar_result(&after_commit, 23, "43");
    let notifications = after_commit
        .iter()
        .filter(|(tag, _)| *tag == b'A')
        .map(|(_, payload)| notification_fields(payload))
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].0 > 0);
    assert_eq!(notifications[0].1, "issue_822_wire_events");
    assert_eq!(notifications[0].2, "committed payload");
    assert_scalar_result(&recovery, 23, "44");
}

#[test]
fn should_apply_subscription_state_changes_at_transaction_commit_over_pgwire() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-listen-unlisten-transaction-pgwire");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (
        begin_listen,
        listen_pending,
        publish_before_listen_commit,
        before_listen_commit,
        commit_listen,
        publish_after_listen_commit,
        after_listen_commit,
        begin_unlisten_rollback,
        unlisten_rollback,
        rollback_unlisten,
        publish_after_unlisten_rollback,
        after_unlisten_rollback,
        begin_unlisten_commit,
        unlisten_commit,
        commit_unlisten,
        publish_after_unlisten_commit,
        after_unlisten_commit,
        recovery,
    ) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie
            .startup()
            .expect("start transactional LISTEN fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut listener_reader, mut listener_writer) = connect_session(&server).await;
        let (mut publisher_reader, mut publisher_writer) = connect_session(&server).await;
        let channel = "issue_822_transactional_subscription";

        let begin_listen = simple_query(&mut listener_reader, &mut listener_writer, "BEGIN").await;
        let listen_pending = simple_query(
            &mut listener_reader,
            &mut listener_writer,
            &format!("LISTEN {channel}"),
        )
        .await;
        let publish_before_listen_commit = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'before subscription commit'"),
        )
        .await;
        let before_listen_commit =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 48").await;
        let commit_listen =
            simple_query(&mut listener_reader, &mut listener_writer, "COMMIT").await;
        let publish_after_listen_commit = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'after subscription commit'"),
        )
        .await;
        let after_listen_commit =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 49").await;

        let begin_unlisten_rollback =
            simple_query(&mut listener_reader, &mut listener_writer, "BEGIN").await;
        let unlisten_rollback = simple_query(
            &mut listener_reader,
            &mut listener_writer,
            &format!("UNLISTEN {channel}"),
        )
        .await;
        let rollback_unlisten =
            simple_query(&mut listener_reader, &mut listener_writer, "ROLLBACK").await;
        let publish_after_unlisten_rollback = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'after unlisten rollback'"),
        )
        .await;
        let after_unlisten_rollback =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 50").await;

        let begin_unlisten_commit =
            simple_query(&mut listener_reader, &mut listener_writer, "BEGIN").await;
        let unlisten_commit = simple_query(
            &mut listener_reader,
            &mut listener_writer,
            &format!("UNLISTEN {channel}"),
        )
        .await;
        let commit_unlisten =
            simple_query(&mut listener_reader, &mut listener_writer, "COMMIT").await;
        let publish_after_unlisten_commit = simple_query(
            &mut publisher_reader,
            &mut publisher_writer,
            &format!("NOTIFY {channel}, 'after unlisten commit'"),
        )
        .await;
        let after_unlisten_commit =
            simple_query(&mut listener_reader, &mut listener_writer, "SELECT 51").await;
        let recovery = simple_query(&mut listener_reader, &mut listener_writer, "SELECT 52").await;

        drop((
            listener_reader,
            listener_writer,
            publisher_reader,
            publisher_writer,
        ));
        server.stop().await;
        (
            begin_listen,
            listen_pending,
            publish_before_listen_commit,
            before_listen_commit,
            commit_listen,
            publish_after_listen_commit,
            after_listen_commit,
            begin_unlisten_rollback,
            unlisten_rollback,
            rollback_unlisten,
            publish_after_unlisten_rollback,
            after_unlisten_rollback,
            begin_unlisten_commit,
            unlisten_commit,
            commit_unlisten,
            publish_after_unlisten_commit,
            after_unlisten_commit,
            recovery,
        )
    });
    drop(runtime);

    // Assert
    assert_command(&begin_listen, "BEGIN");
    assert_command(&listen_pending, "LISTEN");
    assert_command(&publish_before_listen_commit, "NOTIFY");
    assert_scalar_result(&before_listen_commit, 23, "48");
    assert!(
        before_listen_commit.iter().all(|(tag, _)| *tag != b'A'),
        "a subscription pending commit must not receive notifications"
    );
    assert_command(&commit_listen, "COMMIT");
    assert_command(&publish_after_listen_commit, "NOTIFY");
    assert_scalar_result(&after_listen_commit, 23, "49");
    let subscribed_notifications = after_listen_commit
        .iter()
        .filter(|(tag, _)| *tag == b'A')
        .map(|(_, payload)| notification_fields(payload))
        .collect::<Vec<_>>();
    assert_eq!(subscribed_notifications.len(), 1);
    assert_eq!(
        subscribed_notifications[0].1,
        "issue_822_transactional_subscription"
    );
    assert_eq!(subscribed_notifications[0].2, "after subscription commit");
    assert_command(&begin_unlisten_rollback, "BEGIN");
    assert_command(&unlisten_rollback, "UNLISTEN");
    assert_command(&rollback_unlisten, "ROLLBACK");
    assert_command(&publish_after_unlisten_rollback, "NOTIFY");
    assert_scalar_result(&after_unlisten_rollback, 23, "50");
    let rollback_notifications = after_unlisten_rollback
        .iter()
        .filter(|(tag, _)| *tag == b'A')
        .map(|(_, payload)| notification_fields(payload))
        .collect::<Vec<_>>();
    assert_eq!(rollback_notifications.len(), 1);
    assert_eq!(rollback_notifications[0].2, "after unlisten rollback");
    assert_command(&begin_unlisten_commit, "BEGIN");
    assert_command(&unlisten_commit, "UNLISTEN");
    assert_command(&commit_unlisten, "COMMIT");
    assert_command(&publish_after_unlisten_commit, "NOTIFY");
    assert_scalar_result(&after_unlisten_commit, 23, "51");
    assert!(
        after_unlisten_commit.iter().all(|(tag, _)| *tag != b'A'),
        "a committed UNLISTEN must suppress later notifications"
    );
    assert_scalar_result(&recovery, 23, "52");
}

#[test]
fn should_enforce_notify_payload_ceiling_over_pgwire() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-notify-payload-limit-pgwire");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (listen, accepted_payload, accepted_delivery, rejected_payload, after_rejection, recovery) =
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
            cassie.startup().expect("start NOTIFY payload fixture");
            let server = wire::spawn_server(cassie).await;
            let (mut listener_reader, mut listener_writer) = connect_session(&server).await;
            let (mut sender_reader, mut sender_writer) = connect_session(&server).await;
            let channel = "issue_822_payload_limit";
            let listen = simple_query(
                &mut listener_reader,
                &mut listener_writer,
                &format!("LISTEN {channel}"),
            )
            .await;
            let accepted_payload = simple_query(
                &mut sender_reader,
                &mut sender_writer,
                &format!("SELECT pg_notify('{channel}', repeat('x', 7999))"),
            )
            .await;
            let accepted_delivery =
                simple_query(&mut listener_reader, &mut listener_writer, "SELECT 53").await;
            let rejected_payload = simple_query(
                &mut sender_reader,
                &mut sender_writer,
                &format!("SELECT pg_notify('{channel}', repeat('x', 8000))"),
            )
            .await;
            let after_rejection =
                simple_query(&mut listener_reader, &mut listener_writer, "SELECT 54").await;
            let recovery = simple_query(&mut sender_reader, &mut sender_writer, "SELECT 55").await;

            drop((
                listener_reader,
                listener_writer,
                sender_reader,
                sender_writer,
            ));
            server.stop().await;
            (
                listen,
                accepted_payload,
                accepted_delivery,
                rejected_payload,
                after_rejection,
                recovery,
            )
        });
    drop(runtime);

    // Assert
    assert_command(&listen, "LISTEN");
    assert_scalar_result(&accepted_payload, 2278, "");
    assert_scalar_result(&accepted_delivery, 23, "53");
    let accepted_notifications = accepted_delivery
        .iter()
        .filter(|(tag, _)| *tag == b'A')
        .map(|(_, payload)| notification_fields(payload))
        .collect::<Vec<_>>();
    assert_eq!(accepted_notifications.len(), 1);
    assert!(accepted_notifications[0].0 > 0);
    assert_eq!(accepted_notifications[0].1, "issue_822_payload_limit");
    assert_eq!(accepted_notifications[0].2.len(), 7999);
    assert!(
        accepted_notifications[0]
            .2
            .chars()
            .all(|character| character == 'x')
    );
    assert_sqlstate(&rejected_payload, "22023");
    let error_message = rejected_payload
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| {
            wire::parse_error_fields(payload)
                .into_iter()
                .find(|(field, _)| *field == 'M')
                .map(|(_, message)| message)
        });
    assert_eq!(error_message.as_deref(), Some("payload string too long"));
    assert!(
        rejected_payload
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'D' | b'C' | b'A')),
        "an oversized payload must not return a row, command completion, or notification"
    );
    assert_scalar_result(&after_rejection, 23, "54");
    assert!(
        after_rejection.iter().all(|(tag, _)| *tag != b'A'),
        "an oversized payload must not reach listeners"
    );
    assert_scalar_result(&recovery, 23, "55");
}
