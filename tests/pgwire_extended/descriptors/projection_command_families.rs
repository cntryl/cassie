//! Extended-protocol descriptors for fixed projection command results.

use cassie::app::{Cassie, CassieSession};
use cassie::midge::adapter::{RowHashRecord, StorageFamily};
use cassie::types::Value;

use crate::support_pgwire as wire;
use crate::support_sql::canonical_test_collection;
use crate::support_type_metadata_contract as fixture;

struct ExpectedColumn {
    name: &'static str,
    oid: i32,
    size: i16,
}

#[test]
fn should_describe_diff_projection_for_requested_portal_format() {
    // Arrange
    let setup = [
        "CREATE TABLE describe_diff_left (title TEXT)",
        "CREATE TABLE describe_diff_right (title TEXT)",
    ];
    let sql = "DIFF PROJECTION describe_diff_left WITH describe_diff_right LIMIT 5";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .collect();

    // Act
    let batches = run_cycles_with_setup(
        "pgwire-diff-projection-describe",
        &setup,
        |cassie, _| {
            let left = canonical_test_collection(cassie, "describe_diff_left");
            let right = canonical_test_collection(cassie, "describe_diff_right");
            cassie
                .midge
                .put_document(
                    &left,
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "a"}),
                )
                .expect("left row");
            cassie
                .midge
                .put_document(
                    &right,
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "b"}),
                )
                .expect("right row");
        },
        cycles,
    );

    // Assert
    let expected = [
        ExpectedColumn {
            name: "row_id",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "change",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "left_digest",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "right_digest",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "state",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "next_cursor",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "complete",
            oid: 16,
            size: 1,
        },
    ];
    assert_described_results(&batches, &expected);
}

#[test]
fn should_describe_compare_projection_for_requested_portal_format() {
    // Arrange
    let setup = ["CREATE TABLE describe_compare (title TEXT)"];
    let sql = "COMPARE PROJECTION describe_compare WITH MANIFEST '{\"root_digest\":\"mismatch\"}'";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .collect();

    // Act
    let batches = run_cycles_with_setup(
        "pgwire-compare-projection-describe",
        &setup,
        |cassie, _| {
            let collection = canonical_test_collection(cassie, "describe_compare");
            cassie
                .midge
                .put_document(
                    &collection,
                    Some("row-1".to_string()),
                    serde_json::json!({"title": "a"}),
                )
                .expect("comparison row");
        },
        cycles,
    );

    // Assert
    let expected = [
        ExpectedColumn {
            name: "target",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "state",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "root_digest",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "manifest_digest",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "mismatches",
            oid: 23,
            size: 4,
        },
        ExpectedColumn {
            name: "report_id",
            oid: 25,
            size: -1,
        },
    ];
    assert_described_results(&batches, &expected);
}

#[test]
fn should_describe_plan_repair_projection_for_requested_portal_format() {
    // Arrange
    let setup = [
        "CREATE TABLE describe_plan_repair (title TEXT)",
        "INSERT INTO describe_plan_repair (title) VALUES ('a')",
    ];
    let sql = "PLAN REPAIR PROJECTION describe_plan_repair SCOPE row";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .collect();

    // Act
    let batches = run_cycles_with_setup(
        "pgwire-plan-repair-projection-describe",
        &setup,
        |cassie, session| {
            corrupt_first_row_hash(cassie, "describe_plan_repair");
            let verification = cassie
                .execute_sql(
                    session,
                    "VERIFY PROJECTION describe_plan_repair MODE hashes_only",
                    Vec::new(),
                )
                .expect("verification should record repairable row finding");
            assert_eq!(verification.rows()[0][6], Value::Bool(true));
        },
        cycles,
    );

    // Assert
    let expected = repair_columns();
    assert_described_results(&batches, &expected);
}

#[test]
fn should_describe_repair_projection_for_requested_portal_format() {
    // Arrange
    let setup = [
        "CREATE TABLE describe_repair_text (title TEXT)",
        "INSERT INTO describe_repair_text (title) VALUES ('a')",
        "CREATE TABLE describe_repair_binary (title TEXT)",
        "INSERT INTO describe_repair_binary (title) VALUES ('b')",
    ];
    let statements = [
        ("REPAIR PROJECTION describe_repair_text SCOPE row", 0),
        ("REPAIR PROJECTION describe_repair_binary SCOPE row", 1),
    ];
    let cycles = statements
        .into_iter()
        .map(|(sql, format)| fixture::execute_cycle(sql, None, format, true))
        .collect();

    // Act
    let batches = run_cycles_with_setup(
        "pgwire-repair-projection-describe",
        &setup,
        |cassie, session| {
            for table in ["describe_repair_text", "describe_repair_binary"] {
                corrupt_first_row_hash(cassie, table);
                let verification = cassie
                    .execute_sql(
                        session,
                        &format!("VERIFY PROJECTION {table} MODE hashes_only"),
                        Vec::new(),
                    )
                    .expect("verification should record repairable row finding");
                assert_eq!(verification.rows()[0][6], Value::Bool(true));
            }
        },
        cycles,
    );

    // Assert
    let expected = repair_columns();
    assert_described_results(&batches, &expected);
    for frames in &batches {
        let rows = wire::data_rows(frames);
        assert_eq!(rows[0][0].as_deref(), Some("completed"));
        assert_eq!(rows[0][7].as_deref(), Some("verified"));
    }
}

fn repair_columns() -> [ExpectedColumn; 11] {
    [
        ExpectedColumn {
            name: "state",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "projection_name",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "scope",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "target_collection",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "action",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "executable",
            oid: 16,
            size: 1,
        },
        ExpectedColumn {
            name: "verification_required",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "post_verification_state",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "report_id",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "affected_objects",
            oid: 25,
            size: -1,
        },
        ExpectedColumn {
            name: "last_error",
            oid: 25,
            size: -1,
        },
    ]
}

fn assert_described_results(batches: &[fixture::Frames], expected: &[ExpectedColumn]) {
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(frames);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, payload)| wire::parse_row_description(payload))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.iter().enumerate() {
            assert_eq!(columns.len(), expected.len());
            for (column, expected) in columns.iter().zip(expected) {
                assert_eq!(column.name, expected.name);
                assert_eq!(column.type_oid, expected.oid);
                assert_eq!(column.type_size, expected.size);
                assert_eq!(column.type_mod, -1);
                assert_eq!(column.format_code, if index == 0 { 0 } else { format });
            }
        }
        let rows = wire::data_rows(frames);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), expected.len());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    }
}

fn corrupt_first_row_hash(cassie: &Cassie, table: &str) {
    let collection = canonical_test_collection(cassie, table);
    let mut row_hash = cassie
        .midge
        .list_row_hashes(&collection)
        .expect("row hashes")
        .into_iter()
        .next()
        .expect("inserted row hash");
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan data family")
        .into_iter()
        .find_map(|(key, value)| {
            let record = serde_json::from_slice::<RowHashRecord>(&value).ok()?;
            (record.collection == collection && record.row_id == row_hash.row_id).then_some(key)
        })
        .expect("row hash key");
    row_hash.state = cassie::midge::adapter::StoredHashState::Stale;
    let mut transaction = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadWrite)
        .expect("row hash transaction");
    transaction
        .put(
            key,
            serde_json::to_vec(&row_hash).expect("serialize stale row hash"),
            None,
        )
        .expect("stage stale row hash");
    transaction
        .commit(cntryl_midge::WriteOptions::sync())
        .expect("commit stale row hash");
}

fn run_cycles_with_setup(
    label: &str,
    setup: &[&str],
    initialize: impl FnOnce(&Cassie, &CassieSession),
    cycles: Vec<fixture::Cycle>,
) -> Vec<fixture::Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let config = crate::support_sql::with_test_root_password(
            cassie::config::CassieRuntimeConfig::from_env().expect("runtime config"),
        );
        let cassie = Cassie::new_in_memory_with_config(config)
            .unwrap_or_else(|error| panic!("create ephemeral fixture {label}: {error}"));
        cassie.startup().expect("startup before table creation");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed finite command fixture");
        }
        initialize(&cassie, &session);
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            wire::write_frames(&mut writer, cycle).await;
            batches.push(
                wire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    })
}
