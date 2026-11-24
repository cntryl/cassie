#[test]
fn should_resolve_unknown_pagination_parameter_types_from_offset_before_limit() {
    // Arrange
    let cases = [
        (
            "SELECT 1 LIMIT CAST($1 AS INT) OFFSET CAST($1 AS BIGINT)",
            20,
        ),
        (
            "SELECT 1 LIMIT CAST($1 AS BIGINT) OFFSET CAST($1 AS INT)",
            23,
        ),
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (sql, inferred) in cases {
        for oid in [0, 705, 23, 20] {
            for format in [0, 1] {
                requests.push((
                    if matches!(oid, 0 | 705) {
                        inferred
                    } else {
                        oid
                    },
                    format,
                ));
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((oid, 0, Some(b"0"))),
                    format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-offset-first", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 16);
    for ((expected_oid, format), frames) in requests.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[expected_oid]);
        // Unaliased expression naming is a separately retained descriptor gap.
        // This regression selects parameter ordering and numeric wire identity.
        fixture::assert_single_column_descriptors(&frames, (23, 4, -1), format, true);
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_command(&frames, b"SELECT 0\0");
    }
}
