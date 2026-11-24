//! A row id containing a NUL byte is refused at write time: the composite key
//! encoding joins components with 0x00 and cannot represent it, so the id would
//! otherwise be stored but read back under a different id.

use super::pair::{with_pair, Pair, COL, ROW};
use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::canonical_relation_name;
use cassie::types::Value;

const NUL_ID: &str = "a\u{0}b";

fn create(pair: &Pair, name: &str, mode: &str) -> String {
    pair.create(name, "body TEXT", "");
    format!("{name}_{mode}")
}

fn zero_rows(pair: &Pair, table: &str) -> bool {
    pair.run(&format!("SELECT _id FROM {table}")).is_empty()
}

fn put_refused(pair: &Pair, name: &str, mode: &str) {
    // Arrange
    let table = create(pair, name, mode);

    // Act
    let result = pair.cassie.midge.put_document(
        &table,
        Some(NUL_ID.to_owned()),
        serde_json::json!({"body": "x"}),
    );

    // Assert
    let error = result.expect_err("a NUL row id must be refused");
    assert_eq!(error.sql_state(), "22021");
    assert!(zero_rows(pair, &table), "refused write left a row behind");
}

#[test]
fn should_refuse_nul_row_id_in_row_table_put_document() {
    with_pair(|pair| put_refused(pair, "nul_put_row", ROW));
}

#[test]
fn should_refuse_nul_row_id_in_column_table_put_document() {
    with_pair(|pair| put_refused(pair, "nul_put_col", COL));
}

#[test]
fn should_refuse_nul_row_id_in_put_documents_batch() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_batch", ROW);

        // Act
        let result = pair.cassie.midge.put_documents(
            &table,
            vec![
                (Some("ok".to_owned()), serde_json::json!({"body": "1"})),
                (Some(NUL_ID.to_owned()), serde_json::json!({"body": "2"})),
            ],
        );

        // Assert
        assert!(result.is_err(), "a NUL row id in a batch must be refused");
        assert!(
            zero_rows(pair, &table),
            "refused batch was partially applied"
        );
    });
}

#[test]
fn should_refuse_nul_row_id_in_fresh_document_load() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_fresh", ROW);

        // Act
        let result = pair.cassie.midge.put_fresh_documents(
            &table,
            vec![(Some(NUL_ID.to_owned()), serde_json::json!({"body": "x"}))],
        );

        // Assert
        let error = result.expect_err("a NUL row id must be refused");
        assert_eq!(error.sql_state(), "22021");
        assert!(zero_rows(pair, &table), "refused load left a row behind");
    });
}

#[test]
fn should_not_take_the_row_id_from_a_rest_json_id_field() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_rest", ROW);
        let body = serde_json::json!({"_id": NUL_ID, "body": "x"}).to_string();

        // Act
        let result = cassie::rest::documents::create(&pair.cassie, &table, body.as_bytes());

        // Assert
        let ids = pair.run(&format!("SELECT _id FROM {table}"));
        let stored_nul_variant = ids
            .iter()
            .flatten()
            .any(|id| *id == Value::String("a.b".into()) || *id == Value::String(NUL_ID.into()));
        assert!(
            !stored_nul_variant,
            "REST create stored a NUL id (result ok: {})",
            result.is_ok()
        );
    });
}

// SQL cannot supply a row id: `INSERT` always generates it and `_id` is not an
// insertable column, so no SQL statement (literal, bound parameter, `chr(0)`,
// COPY) reaches the writer with a caller-chosen id. NUL in SQL text values is
// refused upstream by the string-literal parser (SQLSTATE 22021).
#[test]
fn should_not_accept_an_id_column_in_sql_insert() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_sql_id", ROW);

        // Act
        let state = pair.sqlstate(&format!(
            "INSERT INTO {table} (_id, body) VALUES ('a', 'x')"
        ));

        // Assert
        assert_eq!(state, "42601");
    });
}

#[test]
fn should_refuse_nul_in_escape_string_literal() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_lit", ROW);

        // Act
        let state = pair.sqlstate(&format!("INSERT INTO {table} (body) VALUES (E'a\\x00b')"));

        // Assert
        assert_eq!(state, "22021");
        assert!(zero_rows(pair, &table));
    });
}

#[test]
fn should_refuse_nul_row_id_in_projection_replay() {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, "nul_replay", ROW);
        let batch = ProjectionReplayBatch {
            projection: canonical_relation_name("postgres", "public", &table),
            source_identity: "nul-stream".to_owned(),
            batch_id: "nul-batch".to_owned(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "nul-event".to_owned(),
                checkpoint: "checkpoint-1".to_owned(),
                position: Some(1),
                document_id: NUL_ID.to_owned(),
                payload: Some(serde_json::json!({"body": "x"})),
            }],
        };

        // Act
        let result = pair.cassie.replay_projection_batch(batch);

        // Assert
        assert!(result.is_err(), "a NUL document id must be refused");
        assert!(zero_rows(pair, &table), "refused replay left a row behind");
    });
}

fn round_trips_ids(mode: &str, name: &str) {
    with_pair(|pair| {
        // Arrange
        let table = create(pair, name, mode);
        let ids = [
            "a.b",
            "a.b.c",
            "dash-and_under",
            "sp ace/slash",
            "naïve-日本",
            "é",
        ];

        // Act
        for id in ids {
            pair.cassie
                .midge
                .put_document(&table, Some(id.to_owned()), serde_json::json!({"body": id}))
                .expect("write id");
        }
        let mut stored = pair
            .run(&format!("SELECT _id FROM {table}"))
            .into_iter()
            .flatten()
            .map(|value| match value {
                Value::String(text) => text,
                _ => panic!("row id was not text"),
            })
            .collect::<Vec<_>>();

        // Assert
        let mut expected = ids.map(str::to_owned).to_vec();
        expected.sort();
        stored.sort();
        assert_eq!(stored, expected);
    });
}

#[test]
fn should_round_trip_punctuated_non_ascii_ids_in_row_table() {
    round_trips_ids(ROW, "nul_guard_row");
}

#[test]
fn should_round_trip_punctuated_non_ascii_ids_in_column_table() {
    round_trips_ids(COL, "nul_guard_col");
}
