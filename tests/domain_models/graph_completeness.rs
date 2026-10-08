use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::StorageFamily;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture_with_config;

const NEIGHBORS: &str =
    "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)";

fn two_neighbors(label: &str) -> crate::support_sql_fixture::SqlFixture {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    sql_fixture_with_config(label, &[
        "CREATE GRAPH social",
        "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1), ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2)",
    ], config)
}

#[test]
fn should_reverify_all_redundant_members_after_a_warm_neighborhood_read() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let fixture = two_neighbors("graph-warm-all-members-missing");
    let expected = vec![
        vec![Value::String("e1".to_owned())],
        vec![Value::String("e2".to_owned())],
    ];
    assert_eq!(fixture.rows(NEIGHBORS), expected);
    let entries = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("Data entries");
    let members = entries
        .iter()
        .filter(|(key, value)| value.is_empty() && key.windows(4).any(|part| part == b"\0e1\0"))
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 4);
    for (key, _) in members {
        fixture
            .cassie
            .midge
            .raw_delete(StorageFamily::Data, key)
            .expect("delete redundant member");
    }

    // Act
    let rows = fixture.rows(NEIGHBORS);

    // Assert
    assert_eq!(rows, expected);
    assert_eq!(
        fixture.cassie.metrics()["graph"]["last_fallback_reason"].as_str(),
        Some("incomplete-sidecar-membership")
    );
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_same_count_substituted_adjacency_members() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let fixture = two_neighbors("graph-same-count-substitution");
    let entries = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("Data entries");
    let (key, value) = entries
        .iter()
        .find(|(key, value)| {
            value.is_empty()
                && key.windows(4).any(|part| part == b"\0OE\0")
                && key.windows(4).any(|part| part == b"\0e1\0")
        })
        .expect("typed member");
    let mut substituted = key.clone();
    let offset = substituted
        .windows(4)
        .position(|part| part == b"\0e1\0")
        .expect("edge identity");
    substituted[offset + 1..offset + 3].copy_from_slice(b"g1");
    fixture
        .cassie
        .midge
        .raw_delete(StorageFamily::Data, key)
        .expect("remove true member");
    fixture
        .cassie
        .midge
        .raw_put(StorageFamily::Data, &substituted, value)
        .expect("same-count wrong member");

    // Act
    let rows = fixture.rows(NEIGHBORS);

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::String("e1".to_owned())],
            vec![Value::String("e2".to_owned())]
        ]
    );
    assert_eq!(
        fixture.cassie.metrics()["graph"]["last_fallback_reason"].as_str(),
        Some("incomplete-sidecar-membership")
    );
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_recover_the_lowest_weight_neighbor_after_live_membership_deletion() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let fixture = sql_fixture_with_config(
        "graph-live-missing-membership",
        &[
            "CREATE GRAPH social",
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1), ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2)",
        ],
        config,
    );
    let entries = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("stored Data entries");
    let (manifest_key, manifest_value) = entries
        .iter()
        .find(|(_, value)| {
            serde_json::from_slice::<serde_json::Value>(value).is_ok_and(|value| {
                value.get("format_version").is_some() && value.get("edge_count").is_some()
            })
        })
        .expect("graph manifest");
    let (missing_key, _) = entries
        .iter()
        .find(|(key, value)| {
            value.is_empty()
                && key.windows(4).any(|part| part == b"\0OE\0")
                && key.windows(4).any(|part| part == b"\0e1\0")
        })
        .expect("lowest-weight outbound typed membership");
    fixture
        .cassie
        .midge
        .raw_delete(StorageFamily::Data, missing_key)
        .expect("delete only one derived member");
    assert_eq!(
        fixture
            .cassie
            .midge
            .raw_get(StorageFamily::Data, manifest_key)
            .expect("unchanged manifest"),
        Some(manifest_value.clone())
    );

    // Act
    let rows = fixture.rows(
        "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::String("e1".to_owned())]]);
    let metrics = fixture.cassie.metrics();
    assert_eq!(
        metrics["graph"]["last_fallback_reason"].as_str(),
        Some("incomplete-sidecar-membership")
    );
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_invalid_members_without_returning_partial_results() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    for extra in [false, true] {
        let fixture = two_neighbors("graph-invalid-members");
        let entries = fixture
            .cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("Data entries");
        let (key, _) = entries
            .iter()
            .find(|(key, value)| value.is_empty() && key.windows(4).any(|part| part == b"\0OE\0"))
            .expect("typed member");
        let mut changed = key.clone();
        if extra {
            changed.push(1);
        }
        fixture
            .cassie
            .midge
            .raw_put(
                StorageFamily::Data,
                &changed,
                if extra { b"" } else { b"invalid" },
            )
            .expect("corrupt member");

        // Act
        let rows = fixture.rows(NEIGHBORS);

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![Value::String("e1".to_owned())],
                vec![Value::String("e2".to_owned())]
            ]
        );
        assert_eq!(
            fixture.cassie.metrics()["graph"]["last_fallback_reason"].as_str(),
            Some("incomplete-sidecar-membership")
        );
        assert_eq!(
            fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
    }
}

#[test]
fn should_cancel_during_authoritative_graph_verification() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let fixture = two_neighbors("graph-verification-pages");
    for index in 2..130 {
        fixture.execute(&format!("INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e{index}', 'person', 'alice', 'person', 'node{index}', 'knows', {index})")).expect("seed page boundary");
    }
    for entries in [1, 128, 129, 131, 258] {
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(entries));

        // Act
        let result = fixture.execute(NEIGHBORS);
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(
            matches!(result, Err(cassie::app::CassieError::QueryCancelled)),
            "entry {entries}: {result:?}"
        );
        assert_eq!(
            fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
    }
    assert_eq!(fixture.rows(NEIGHBORS).len(), 10);
}

#[test]
fn should_reject_a_late_corrupt_member_after_warm_verification() {
    // Arrange
    let _scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let fixture = two_neighbors("graph-late-corrupt-member");
    for index in 3..131 {
        fixture.execute(&format!("INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e{index}', 'person', 'alice', 'person', 'node{index}', 'knows', {index})")).expect("seed source boundary");
    }
    let sql = "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)";
    let expected = vec![vec![Value::String("e1".to_owned())]];
    assert_eq!(fixture.rows(sql), expected);
    let entries = fixture
        .cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("Data entries");
    let members = entries
        .iter()
        .filter(|(_, value)| value.is_empty())
        .collect::<Vec<_>>();
    let index = members
        .iter()
        .position(|(key, _)| {
            key.windows(4).any(|part| part == b"\0OE\0")
                && key.windows(4).any(|part| part == b"\0e1\0")
        })
        .expect("wanted outbound member");
    assert!(index > 128, "late member position {index}");
    fixture
        .cassie
        .midge
        .raw_delete(StorageFamily::Data, &members[index].0)
        .expect("remove late member");

    // Act
    let rows = fixture.rows(sql);

    // Assert
    assert_eq!(rows, expected);
    let metrics = fixture.cassie.metrics();
    assert_eq!(
        metrics["graph"]["last_fallback_reason"].as_str(),
        Some("incomplete-sidecar-membership")
    );
    assert!(
        metrics["graph"]["last_reads"]
            .as_u64()
            .expect("counted reads")
            > 128
    );
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}
