//! Extended-protocol descriptors for projection verification commands.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_describe_verify_projection_with_requested_portal_format() {
    // Arrange
    let setup = [("CREATE TABLE output_verification (n INT)", Vec::new())];
    let sql = "VERIFY PROJECTION output_verification MODE metadata_only";
    let cycles = vec![
        fixture::execute_cycle(sql, None, 0, true),
        fixture::execute_cycle(sql, None, 1, true),
    ];
    let expected = [
        ("state", 25, -1, -1),
        ("target_collection", 25, -1, -1),
        ("mode", 25, -1, -1),
        ("mismatch_count", 20, 8, -1),
        ("missing_count", 20, 8, -1),
        ("stale_count", 20, 8, -1),
        ("repairable", 16, 1, -1),
        ("checked_components", 25, -1, -1),
        ("skipped_components", 25, -1, -1),
        ("last_error", 25, -1, -1),
    ];

    // Act
    let batches = fixture::run_cycles("pgwire-verify-projection-describe", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, payload)| wire::parse_row_description(payload))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.iter().enumerate() {
            assert_eq!(columns.len(), expected.len());
            for (column, (name, oid, size, modifier)) in columns.iter().zip(expected) {
                assert_eq!(
                    (
                        column.name.as_str(),
                        column.type_oid,
                        column.type_size,
                        column.type_mod,
                        column.format_code,
                    ),
                    (
                        name,
                        oid,
                        size,
                        modifier,
                        if index == 0 { 0 } else { format },
                    )
                );
            }
        }
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), expected.len());
        assert_eq!(rows[0][2].as_deref(), Some("metadata_only"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    }
}
