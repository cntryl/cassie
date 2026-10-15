//! PostgreSQL 18.6 EXPLAIN option-list and JSON wire behavior for #821.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[test]
fn should_execute_verbose_json_explain_option_list_over_extended_protocol() {
    // Arrange
    let sql = "EXPLAIN (ANALYZE FALSE, VERBOSE TRUE, COSTS FALSE, FORMAT JSON) SELECT probe_value FROM issue_821_explain_options WHERE probe_value = 17";
    let setup = [
        (
            "CREATE TABLE issue_821_explain_options (probe_value INT)",
            Vec::<Value>::new(),
        ),
        (
            "INSERT INTO issue_821_explain_options VALUES (17),(18)",
            Vec::<Value>::new(),
        ),
    ];

    // Act
    let batches = fixture::run_cycles(
        "issue-821-explain-json-options",
        &setup,
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    let frames = batches.first().expect("EXPLAIN extended-query response");

    // Assert
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert!(frames.iter().any(|(tag, _)| *tag == b'2'));
    let rows = wire::data_rows(frames);
    assert_eq!(rows.len(), 1);
    let plan_json = rows[0][0].as_deref().expect("JSON EXPLAIN plan row");
    let document: serde_json::Value =
        serde_json::from_str(plan_json).expect("EXPLAIN FORMAT JSON output");
    let plan = document
        .as_array()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.get("Plan"))
        .expect("one JSON plan object");

    assert_eq!(plan["Node Type"], "Seq Scan");
    assert_eq!(plan["Relation Name"], "issue_821_explain_options");
    assert_eq!(plan["Output"], serde_json::json!(["probe_value"]));
    assert_eq!(
        plan["Filter"],
        "(issue_821_explain_options.probe_value = 17)"
    );
    assert!(plan.get("Startup Cost").is_none());
    assert!(plan.get("Total Cost").is_none());
    assert!(plan.get("Actual Rows").is_none());
}
