//! Duplicate-key SQLSTATE contracts for PostgreSQL 18 unique JSON aggregates.

use std::time::Duration;

use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

fn run_queries(label: &str, setup: &[&str], queries: &[&str]) -> Vec<Frames> {
    let path = wire::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let batches = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("start Cassie before setup");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed duplicate-key aggregate fixture");
        }

        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect to Cassie pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut batches = Vec::with_capacity(queries.len());
        for sql in queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });

    let _ = std::fs::remove_dir_all(path);
    batches
}

#[test]
fn should_reject_duplicate_keys_in_unique_json_object_aggregates_with_sqlstate() {
    // Arrange
    let setup = [
        "CREATE TABLE issue_811_unique_json_duplicate_values (key TEXT, value INT)",
        "INSERT INTO issue_811_unique_json_duplicate_values VALUES ('same', 1), ('same', 2)",
    ];
    let cases = [
        (
            "json_object_agg_unique",
            "SELECT json_object_agg_unique(key, value) FROM issue_811_unique_json_duplicate_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique",
            "SELECT jsonb_object_agg_unique(key, value) FROM issue_811_unique_json_duplicate_values",
            "duplicate JSON object key value",
        ),
        (
            "json_object_agg_unique_strict",
            "SELECT json_object_agg_unique_strict(key, value) FROM issue_811_unique_json_duplicate_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique_strict",
            "SELECT jsonb_object_agg_unique_strict(key, value) FROM issue_811_unique_json_duplicate_values",
            "duplicate JSON object key value",
        ),
    ];
    let queries = cases
        .iter()
        .map(|(_, sql, _)| *sql)
        .chain(std::iter::once("SELECT 42"))
        .collect::<Vec<_>>();

    // Act
    let batches = run_queries("issue-811-unique-json-duplicate-keys", &setup, &queries);

    // Assert
    let mut mismatches = Vec::new();
    for ((name, _, expected_message), frames) in cases.iter().zip(&batches) {
        let actual_code = wire::error_code(frames);
        if actual_code.as_deref() != Some("22030") {
            mismatches.push(format!("{name} returned SQLSTATE {actual_code:?}"));
            continue;
        }
        let actual_message = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .and_then(|(_, body)| {
                wire::parse_error_fields(body)
                    .into_iter()
                    .find_map(|(key, value)| (key == 'M').then_some(value))
            });
        if actual_message.as_deref() != Some(*expected_message) {
            mismatches.push(format!("{name} returned message {actual_message:?}"));
        }
        if frames
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
        {
            mismatches.push(format!("{name} published result frames after an error"));
        }
    }

    let recovery = batches.last().expect("post-error query batch");
    assert_eq!(
        wire::error_code(recovery),
        None,
        "connection recovery query"
    );
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
    assert!(
        mismatches.is_empty(),
        "unique JSON aggregate duplicate-key mismatches: {mismatches:?}"
    );
}
