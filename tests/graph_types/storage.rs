//! Storage of the reserved types in table columns, across a local restart.

use crate::support_composite_types::LocalStore;
use cassie::types::Value;

fn run(cassie: &cassie::app::Cassie, sql: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    match cassie.execute_sql(&session, sql, Vec::new()) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("query failed: {sql}: {error}"),
    }
}

#[test]
fn should_restore_stored_graph_values_after_a_restart() {
    // Arrange
    let store = LocalStore::new("graph_types_restart");
    let first = store.open();
    run(&first, "CREATE TABLE people (id BIGINT PRIMARY KEY)");
    run(&first, "INSERT INTO people VALUES (9007199254740993)");
    run(
        &first,
        "CREATE PROPERTY GRAPH g VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id)) \
         EDGE TABLES (people AS loop KEY (id) SOURCE KEY (id) REFERENCES person (id) \
         DESTINATION KEY (id) REFERENCES person (id) LABEL loop PROPERTIES (id))",
    );
    run(
        &first,
        "CREATE TABLE holders (id INT PRIMARY KEY, r cassie_graph.graph_element_ref)",
    );
    run(
        &first,
        "INSERT INTO holders VALUES (1, cassie_graph.vertex_ref('g', 'person', \
         ARRAY[cassie_graph.key_part(9007199254740993::bigint)]))",
    );
    let before = run(&first, "SELECT r FROM holders");
    let type_before = run(
        &first,
        "SELECT oid FROM pg_type WHERE typname = 'graph_element_ref'",
    );
    drop(first);

    // Act
    let second = store.open();
    let after = run(&second, "SELECT r FROM holders");
    let type_after = run(
        &second,
        "SELECT oid FROM pg_type WHERE typname = 'graph_element_ref'",
    );
    let key = run(
        &second,
        "SELECT ((r).key_parts[1]).integer_value FROM holders",
    );

    // Assert
    assert_eq!(before, after);
    assert_eq!(type_before, type_after);
    assert_eq!(key, vec![vec![Value::Int64(9_007_199_254_740_993)]]);
}

#[test]
fn should_reject_an_invalid_value_written_to_a_stored_column() {
    // Arrange
    let fixture = crate::fixture::started("graph_types_column", &[]);
    fixture.run("CREATE TABLE parts (id INT PRIMARY KEY, p cassie_graph.graph_key_part)");

    // Act
    let state = fixture.sqlstate("INSERT INTO parts VALUES (1, '(4,,,)')");

    // Assert
    assert_eq!(state, "22023");
}

#[test]
fn should_reject_an_invalid_value_written_by_update() {
    // Arrange
    let fixture = crate::fixture::started("graph_types_update", &[]);
    fixture.run("CREATE TABLE parts (id INT PRIMARY KEY, p cassie_graph.graph_key_part)");
    fixture.run("INSERT INTO parts VALUES (1, '(3,1,,)')");

    // Act
    let state = fixture.sqlstate("UPDATE parts SET p = '(3,,,)' WHERE id = 1");

    // Assert
    assert_eq!(state, "22023");
}

fn replay_key_part(
    cassie: &cassie::app::Cassie,
    document_id: &str,
    fields: serde_json::Value,
) -> Result<u64, cassie::app::CassieError> {
    let oid = match run(
        cassie,
        "SELECT oid::text FROM pg_type WHERE typname = 'graph_key_part'",
    )
    .first()
    .and_then(|row| row.first())
    {
        Some(Value::String(text)) => text.parse::<u32>().expect("type OID"),
        _ => panic!("graph_key_part has no pg_type row"),
    };
    cassie
        .replay_projection_batch(cassie::app::ProjectionReplayBatch {
            projection: cassie.midge.canonical_collection_name("replayed_parts"),
            source_identity: "parts".to_owned(),
            batch_id: format!("batch-{document_id}"),
            lag: 0,
            events: vec![cassie::app::ProjectionReplayEvent {
                event_id: format!("event-{document_id}"),
                checkpoint: format!("checkpoint-{document_id}"),
                position: None,
                document_id: document_id.to_owned(),
                payload: Some(serde_json::json!({
                    "part": { "$cassie_record_v1": { "oid": oid, "fields": fields } }
                })),
            }],
        })
        .map(|report| report.applied_event_count)
}

#[test]
fn should_validate_reserved_values_written_by_projector_replay() {
    // Arrange
    let config = cassie::config::CassieRuntimeConfig::default();
    let cassie = cassie::app::Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    run(
        &cassie,
        "CREATE TABLE replayed_parts (part cassie_graph.graph_key_part)",
    );

    // Act
    let valid = replay_key_part(&cassie, "ok", serde_json::json!([3, 7, null, null]));
    let wrong_slot = replay_key_part(&cassie, "slot", serde_json::json!([3, null, "seven", null]));
    let two_slots = replay_key_part(&cassie, "two", serde_json::json!([3, 7, "seven", null]));
    let unknown_tag = replay_key_part(&cassie, "tag", serde_json::json!([9, 7, null, null]));
    let stored = run(&cassie, "SELECT count(*)::text FROM replayed_parts");

    // Assert
    assert_eq!(valid.ok(), Some(1));
    assert!(wrong_slot.is_err());
    assert!(two_slots.is_err());
    assert!(unknown_tag.is_err());
    assert_eq!(stored, vec![vec![Value::String("1".to_owned())]]);
}

#[test]
fn should_validate_a_reserved_value_nested_in_a_user_type() {
    // Arrange
    let fixture = crate::fixture::started(
        "graph_types_nested",
        &[
            "CREATE TYPE tagged AS (label text, part cassie_graph.graph_key_part)",
            "CREATE TABLE tagged_rows (id int PRIMARY KEY, item tagged, parts cassie_graph.graph_key_part[])",
        ],
    );

    // Act
    let nested_cast = fixture.sqlstate("SELECT ROW('x', ROW(3, NULL, 'seven', NULL))::tagged");
    let nested_insert = fixture
        .sqlstate("INSERT INTO tagged_rows VALUES (1, ROW('x', ROW(9, 7, NULL, NULL)), NULL)");
    let array_insert = fixture.sqlstate(
        "INSERT INTO tagged_rows VALUES (2, NULL, ARRAY[ROW(3, 7, 'seven', NULL)::cassie_graph.graph_key_part])",
    );
    let valid = fixture.execute(
        "INSERT INTO tagged_rows VALUES (3, ROW('x', cassie_graph.key_part(7::bigint)), \
         ARRAY[cassie_graph.key_part('seven'::text)])",
    );

    // Assert
    assert_eq!(nested_cast, "22023");
    assert_eq!(nested_insert, "22023");
    assert_eq!(array_insert, "22023");
    assert!(valid.is_ok());
}
