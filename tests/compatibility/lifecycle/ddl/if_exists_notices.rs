use super::*;
use std::time::Duration;

async fn simple_query_frames(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await
}

async fn simple_query_sequence_frames(
    server: &CompatibilityServer,
    statements: &[&str],
) -> Vec<Vec<(u8, Vec<u8>)>> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw simple-query client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    let mut responses = Vec::with_capacity(statements.len());
    for sql in statements {
        support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)])
            .await;
        responses.push(
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await,
        );
    }
    responses
}

async fn extended_query_frames(
    server: &CompatibilityServer,
    statement_name: &str,
    portal_name: &str,
    sql: &str,
) -> Vec<(u8, Vec<u8>)> {
    extended_query_sequence_frames(server, &[(statement_name, portal_name, sql)])
        .await
        .into_iter()
        .next()
        .expect("one extended-query response")
}

async fn extended_query_sequence_frames(
    server: &CompatibilityServer,
    statements: &[(&str, &str, &str)],
) -> Vec<Vec<(u8, Vec<u8>)>> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw extended-query client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    let mut responses = Vec::with_capacity(statements.len());
    for (statement_name, portal_name, sql) in statements {
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::parse_frame(statement_name, sql),
                support_pgwire::bind_frame(portal_name, statement_name, &[]),
                support_pgwire::execute_frame(portal_name),
                support_pgwire::sync_frame(),
            ],
        )
        .await;
        responses.push(
            support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                .await,
        );
    }
    responses
}

fn command_tag(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

fn notice_fields(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<(char, String)>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'N')
        .map(|(_, payload)| support_pgwire::parse_error_fields(payload))
        .collect()
}

fn notice_field(fields: &[(char, String)], kind: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field_kind, _)| *field_kind == kind)
        .map(|(_, value)| value.as_str())
}

#[test]
fn should_emit_if_exists_notices_with_command_tags() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_if_exists_notices").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_drop_present (id INT)",
                "CREATE TABLE compat_issue839_alter_present (id INT)",
            ],
        )
        .await
        .expect("create present objects for IF EXISTS comparisons");

        // Act
        let absent_drop =
            simple_query_frames(&server, "DROP TABLE IF EXISTS compat_issue839_drop_absent").await;
        let present_drop =
            simple_query_frames(&server, "DROP TABLE IF EXISTS compat_issue839_drop_present").await;
        let absent_alter = simple_query_frames(
            &server,
            "ALTER TABLE IF EXISTS compat_issue839_alter_absent ADD COLUMN ignored INT",
        )
        .await;
        let present_alter = simple_query_frames(
            &server,
            "ALTER TABLE IF EXISTS compat_issue839_alter_present \
             ADD COLUMN added INT DEFAULT 7",
        )
        .await;
        // Assert
        for (statement, frames) in [
            ("absent DROP TABLE", &absent_drop),
            ("present DROP TABLE", &present_drop),
            ("absent ALTER TABLE", &absent_alter),
            ("present ALTER TABLE", &present_alter),
        ] {
            assert_eq!(
                support_pgwire::error_code(frames),
                None,
                "{statement} frames: {frames:?}"
            );
        }
        let absent_drop_notices = notice_fields(&absent_drop);
        assert_eq!(absent_drop_notices.len(), 1);
        assert_eq!(notice_field(&absent_drop_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_drop_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_drop_notices[0], 'M'),
            Some("table \"compat_issue839_drop_absent\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_drop).as_deref(), Some("DROP TABLE"));
        assert_eq!(
            notice_fields(&present_drop),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_drop).as_deref(), Some("DROP TABLE"));

        let absent_alter_notices = notice_fields(&absent_alter);
        assert_eq!(absent_alter_notices.len(), 1);
        assert_eq!(notice_field(&absent_alter_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_alter_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_alter_notices[0], 'M'),
            Some("relation \"compat_issue839_alter_absent\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_alter).as_deref(), Some("ALTER TABLE"));
        assert_eq!(
            notice_fields(&present_alter),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_alter).as_deref(), Some("ALTER TABLE"));
        let inserted = client
            .query_one(
                "INSERT INTO compat_issue839_alter_present (id) VALUES (1) RETURNING added",
                &[],
            )
            .await
            .expect("verify that the present IF EXISTS ALTER ran");
        assert_eq!(inserted.get::<_, i32>(0), 7);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_emit_if_exists_notice_during_extended_query_execution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_extended_if_exists_notice").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_issue839_extended_drop_present (id INT)")
            .await
            .expect("create relation for the present DROP IF EXISTS case");
        client
            .batch_execute("CREATE TABLE compat_issue839_extended_alter_present (id INT)")
            .await
            .expect("create relation for the present ALTER IF EXISTS case");

        // Act
        let absent_drop = extended_query_frames(
            &server,
            "issue839_absent_drop_stmt",
            "issue839_absent_drop_portal",
            "DROP TABLE IF EXISTS compat_issue839_drop_absent_extended",
        )
        .await;
        let present_drop = extended_query_frames(
            &server,
            "issue839_present_drop_stmt",
            "issue839_present_drop_portal",
            "DROP TABLE IF EXISTS compat_issue839_extended_drop_present",
        )
        .await;
        let absent_alter = extended_query_frames(
            &server,
            "issue839_absent_alter_stmt",
            "issue839_absent_alter_portal",
            "ALTER TABLE IF EXISTS compat_issue839_extended_alter_absent ADD COLUMN ignored INT",
        )
        .await;
        let present_alter = extended_query_frames(
            &server,
            "issue839_present_alter_stmt",
            "issue839_present_alter_portal",
            "ALTER TABLE IF EXISTS compat_issue839_extended_alter_present \
             ADD COLUMN added INT DEFAULT 7",
        )
        .await;

        // Assert
        for (statement, frames) in [
            ("absent DROP TABLE", &absent_drop),
            ("present DROP TABLE", &present_drop),
            ("absent ALTER TABLE", &absent_alter),
            ("present ALTER TABLE", &present_alter),
        ] {
            assert_eq!(
                support_pgwire::error_code(frames),
                None,
                "{statement} frames: {frames:?}"
            );
        }
        let absent_drop_notices = notice_fields(&absent_drop);
        assert_eq!(absent_drop_notices.len(), 1);
        assert_eq!(notice_field(&absent_drop_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_drop_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_drop_notices[0], 'M'),
            Some("table \"compat_issue839_drop_absent_extended\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_drop).as_deref(), Some("DROP TABLE"));
        assert_eq!(
            notice_fields(&present_drop),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_drop).as_deref(), Some("DROP TABLE"));
        let absent_alter_notices = notice_fields(&absent_alter);
        assert_eq!(absent_alter_notices.len(), 1);
        assert_eq!(notice_field(&absent_alter_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_alter_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_alter_notices[0], 'M'),
            Some("relation \"compat_issue839_extended_alter_absent\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_alter).as_deref(), Some("ALTER TABLE"));
        assert_eq!(
            notice_fields(&present_alter),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_alter).as_deref(), Some("ALTER TABLE"));
        let dropped = client
            .query_one(
                "SELECT to_regclass('compat_issue839_extended_drop_present')::TEXT",
                &[],
            )
            .await
            .expect("confirm the present extended-query DROP ran");
        assert_eq!(dropped.get::<_, Option<String>>(0), None);
        let inserted = client
            .query_one(
                "INSERT INTO compat_issue839_extended_alter_present (id) VALUES (1) RETURNING added",
                &[],
            )
            .await
            .expect("verify that the present extended-query ALTER ran");
        assert_eq!(inserted.get::<_, i32>(0), 7);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_notice_when_extended_query_alters_a_relation_created_in_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_if_exists_transaction_overlay").await;
        let (client, connection) = server.connect().await;

        // Act
        let responses = extended_query_sequence_frames(
            &server,
            &[
                (
                    "issue839_tx_begin_stmt",
                    "issue839_tx_begin_portal",
                    "BEGIN",
                ),
                (
                    "issue839_tx_create_stmt",
                    "issue839_tx_create_portal",
                    "CREATE TABLE compat_issue839_tx_created (id INT)",
                ),
                (
                    "issue839_tx_alter_stmt",
                    "issue839_tx_alter_portal",
                    "ALTER TABLE IF EXISTS compat_issue839_tx_created \
                     ADD COLUMN added INT DEFAULT 7",
                ),
                (
                    "issue839_tx_commit_stmt",
                    "issue839_tx_commit_portal",
                    "COMMIT",
                ),
            ],
        )
        .await;

        // Assert
        assert_eq!(responses.len(), 4);
        for (statement, frames) in [
            ("BEGIN", &responses[0]),
            ("CREATE TABLE", &responses[1]),
            ("ALTER TABLE", &responses[2]),
            ("COMMIT", &responses[3]),
        ] {
            assert_eq!(
                support_pgwire::error_code(frames),
                None,
                "{statement} frames: {frames:?}"
            );
        }
        assert_eq!(
            notice_fields(&responses[2]),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&responses[2]).as_deref(), Some("ALTER TABLE"));
        let inserted = client
            .query_one(
                "INSERT INTO compat_issue839_tx_created (id) VALUES (1) RETURNING added",
                &[],
            )
            .await
            .expect("verify transaction-local relation alteration was committed");
        assert_eq!(inserted.get::<_, i32>(0), 7);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_notice_when_simple_query_alters_a_relation_created_in_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server =
            CompatibilityServer::start("issue_839_simple_if_exists_transaction_overlay").await;
        let (client, connection) = server.connect().await;

        // Act
        let responses = simple_query_sequence_frames(
            &server,
            &[
                "BEGIN",
                "CREATE TABLE compat_issue839_simple_tx_created (id INT)",
                "ALTER TABLE IF EXISTS compat_issue839_simple_tx_created \
                 ADD COLUMN added INT DEFAULT 7",
                "COMMIT",
            ],
        )
        .await;

        // Assert
        assert_eq!(responses.len(), 4);
        for (statement, frames) in [
            ("BEGIN", &responses[0]),
            ("CREATE TABLE", &responses[1]),
            ("ALTER TABLE", &responses[2]),
            ("COMMIT", &responses[3]),
        ] {
            assert_eq!(
                support_pgwire::error_code(frames),
                None,
                "{statement} frames: {frames:?}"
            );
        }
        assert_eq!(
            notice_fields(&responses[2]),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&responses[2]).as_deref(), Some("ALTER TABLE"));
        let inserted = client
            .query_one(
                "INSERT INTO compat_issue839_simple_tx_created (id) VALUES (1) RETURNING added",
                &[],
            )
            .await
            .expect("verify simple-query transaction-local relation alteration was committed");
        assert_eq!(inserted.get::<_, i32>(0), 7);
        drop(client);
        server.shutdown(connection).await;
    });
}
