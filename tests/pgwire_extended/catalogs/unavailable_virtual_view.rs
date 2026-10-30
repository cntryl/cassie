//! Unavailable `pg_catalog` relations fail as undefined tables and leave pgwire reusable.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_recover_after_undefined_pg_catalog_view_lookup() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM pg_catalog.pg_issue_777_missing_virtual_view",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7::INT")]),
    ];

    // Act
    let batches = run_script("unavailable-pg-catalog-view", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    let failed = &batches[0];
    assert_eq!(wire::error_code(failed).as_deref(), Some("42P01"));
    assert_eq!(
        failed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'E', b'Z']
    );
    assert_eq!(
        failed.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(b"7")]
    );
}
