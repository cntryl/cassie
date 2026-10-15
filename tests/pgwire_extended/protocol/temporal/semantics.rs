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

include!("../../types/temporal/binary.rs");

include!("../../types/temporal/errors.rs");

include!("../../types/temporal/metadata.rs");

include!("../../types/temporal/parameters.rs");

include!("../../types/temporal/recovery.rs");

include!("../../types/temporal/semantics.rs");
