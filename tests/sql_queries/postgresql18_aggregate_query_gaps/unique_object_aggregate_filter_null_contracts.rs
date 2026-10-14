//! FILTER, ordering, strict NULL omission, and unique-key checks for JSON objects.

use cassie::app::Cassie;
use cassie::types::Value;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture;

type Frames = Vec<(u8, Vec<u8>)>;

fn run_queries(label: &str, setup: &[&str], queries: &[&str]) -> Vec<Frames> {
    wire::use_local_storage();
    let path = wire::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");

    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start Cassie before setup");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed unique JSON aggregate fixture");
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
fn should_filter_excluded_duplicates_before_ordered_unique_strict_json_aggregation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-filtered-unique-strict-json-objects",
        &[
            "CREATE TABLE issue_811_filtered_unique_json_values (position INT, key TEXT, value INT, included BOOLEAN)",
            "INSERT INTO issue_811_filtered_unique_json_values VALUES (1,'z',1,TRUE),(2,'a',2,FALSE),(3,'a',3,TRUE),(4,'z',9,FALSE),(5,'omit',NULL,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT json_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included)::TEXT, \
                pg_typeof(json_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included))::OID, \
                jsonb_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included)::TEXT, \
                pg_typeof(jsonb_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included))::OID \
         FROM issue_811_filtered_unique_json_values",
    );

    // Assert
    let result = result.expect("FILTER excludes duplicate keys before unique validation");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{ \"z\" : 1, \"a\" : 3 }".to_owned()),
            Value::Int64(114),
            Value::String("{\"a\": 3, \"z\": 1}".to_owned()),
            Value::Int64(3802),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 26, 25, 26]
    );
}

#[test]
fn should_reject_duplicate_sql_null_values_in_unique_json_object_aggregates() {
    // Arrange
    let setup = [
        "CREATE TABLE issue_811_unique_json_null_values (key TEXT, value INT, position INT)",
        "INSERT INTO issue_811_unique_json_null_values VALUES ('same',NULL,1),('same',NULL,2),('kept',9,3)",
    ];
    let cases = [
        (
            "json_object_agg_unique",
            "SELECT json_object_agg_unique(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique",
            "SELECT jsonb_object_agg_unique(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value",
        ),
        (
            "json_object_agg_unique_strict",
            "SELECT json_object_agg_unique_strict(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique_strict",
            "SELECT jsonb_object_agg_unique_strict(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value",
        ),
    ];
    let queries = cases
        .iter()
        .map(|(_, sql, _)| *sql)
        .chain(std::iter::once("SELECT 42"))
        .collect::<Vec<_>>();

    // Act
    let batches = run_queries("issue-811-unique-json-null-duplicates", &setup, &queries);

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
        "unique JSON aggregate NULL duplicate mismatches: {mismatches:?}"
    );
}
