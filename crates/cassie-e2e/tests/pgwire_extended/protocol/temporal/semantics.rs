//! PostgreSQL 18 temporal descriptors, codecs, settings, and errors tracked by #814.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../../types/temporal/infinity_arithmetic_edges.rs"]
mod infinity_arithmetic_edges;
#[path = "../../aggregates/temporal/interval_aggregate_metadata.rs"]
mod interval_aggregate_metadata;
#[path = "../../types/temporal/time_twenty_four_wire_endpoint.rs"]
mod time_twenty_four_wire_endpoint;

fn binary_row(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<u8>> {
    let payload = &frames
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("DataRow")
        .1;
    let mut cursor = 0_usize;
    let field_count =
        i16::from_be_bytes(payload[cursor..cursor + 2].try_into().expect("field count"));
    cursor += 2;
    let mut fields = Vec::new();
    for _ in 0..field_count {
        let length = i32::from_be_bytes(
            payload[cursor..cursor + 4]
                .try_into()
                .expect("field length"),
        );
        cursor += 4;
        assert!(length >= 0, "temporal output should not be NULL");
        let end = cursor + usize::try_from(length).expect("nonnegative field length");
        fields.push(payload[cursor..end].to_vec());
        cursor = end;
    }
    assert_eq!(cursor, payload.len(), "DataRow has no trailing bytes");
    fields
}

#[test]
fn should_accept_postgresql_datestyle_session_modes() {
    // Arrange
    let steps = [
        "SET DateStyle = 'ISO, MDY'",
        "SET DateStyle = 'SQL, DMY'",
        "SET DateStyle = 'Postgres, DMY'",
        "SET DateStyle = 'German, DMY'",
    ]
    .into_iter()
    .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
    .collect();

    // Act
    let batches = run_script("postgresql18-datestyle-session-modes", steps);

    // Assert
    for (index, batch) in batches.iter().enumerate() {
        assert_eq!(
            wire::error_code(batch),
            None,
            "batch {index}: {:?}",
            batch
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn should_send_datestyle_status_before_batched_simple_query_rows() {
    // Arrange
    let batches = run_script(
        "datestyle-status-before-batched-rows",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SET DateStyle = 'SQL, DMY'; SELECT CURRENT_DATE AS value",
        )])],
    );

    // Act
    let batch = batches.first().expect("batched simple query response");

    // Assert
    assert_eq!(wire::error_code(batch), None);
    assert_datestyle_status_before_rows(batch, 0);
}

#[test]
fn should_send_datestyle_status_after_rows_from_a_view_query() {
    // Arrange
    let batches = run_script(
        "datestyle-status-after-view-rows",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SET DateStyle = 'ISO, MDY'",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE VIEW datestyle_status_view AS SELECT set_config('datestyle', 'SQL, DMY', false) AS changed",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT changed, CURRENT_DATE AS value FROM datestyle_status_view",
            )]),
        ],
    );

    // Act
    let batch = batches.get(2).expect("view query response");

    // Assert
    assert_eq!(wire::error_code(batch), None);
    assert_datestyle_status_after_rows(batch, 1);
}

#[test]
fn should_send_datestyle_status_after_rows_from_an_extended_query() {
    // Arrange
    let batches = run_script(
        "datestyle-status-after-extended-rows",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SET DateStyle = 'ISO, MDY'",
            )]),
            Step::ThroughReady(vec![
                wire::parse_frame(
                    "datestyle_status_statement",
                    "SELECT set_config('datestyle', 'SQL, DMY', false) AS changed, CURRENT_DATE AS value",
                ),
                wire::bind_frame("datestyle_status_portal", "datestyle_status_statement", &[]),
                wire::execute_frame("datestyle_status_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let batch = batches.get(1).expect("extended query response");

    // Assert
    assert_eq!(wire::error_code(batch), None);
    assert_datestyle_status_after_rows(batch, 1);
}

fn assert_datestyle_status_before_rows(batch: &[(u8, Vec<u8>)], date_index: usize) {
    let status_index = datestyle_status_index(batch);
    let row_index = data_row_index(batch);
    assert!(
        status_index < row_index,
        "SET status must precede rows from the following statement in the same query"
    );
    let row = wire::parse_data_row(&batch[row_index].1);
    let date = row.get(date_index).and_then(Option::as_deref).unwrap_or("");
    assert!(date.contains('/'), "expected SQL, DMY text date: {row:?}");
}

fn assert_datestyle_status_after_rows(batch: &[(u8, Vec<u8>)], date_index: usize) {
    let status_index = datestyle_status_index(batch);
    let row_index = data_row_index(batch);
    assert!(
        row_index < status_index,
        "a query's setting change must follow rows encoded under the statement-start style"
    );
    let row = wire::parse_data_row(&batch[row_index].1);
    let date = row.get(date_index).and_then(Option::as_deref).unwrap_or("");
    assert!(date.contains('-'), "expected ISO text date: {row:?}");
}

fn datestyle_status_index(batch: &[(u8, Vec<u8>)]) -> usize {
    batch
        .iter()
        .position(|(tag, payload)| {
            *tag == b'S' && parameter_status(payload) == ("DateStyle", "SQL, DMY")
        })
        .expect("DateStyle ParameterStatus update")
}

fn data_row_index(batch: &[(u8, Vec<u8>)]) -> usize {
    batch
        .iter()
        .position(|(tag, _)| *tag == b'D')
        .expect("DataRow")
}

fn parameter_status(payload: &[u8]) -> (&str, &str) {
    let separator = payload
        .iter()
        .position(|byte| *byte == 0)
        .expect("ParameterStatus key terminator");
    let value = &payload[separator + 1..payload.len().saturating_sub(1)];
    (
        std::str::from_utf8(&payload[..separator]).expect("ParameterStatus key"),
        std::str::from_utf8(value).expect("ParameterStatus value"),
    )
}

include!("../../types/temporal/binary.rs");

include!("../../types/temporal/errors.rs");

include!("../../types/temporal/metadata.rs");

include!("../../types/temporal/parameters.rs");

include!("../../types/temporal/recovery.rs");

include!("../../types/temporal/semantics.rs");
