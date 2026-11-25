#[test]
fn should_finalize_unknown_empty_output_as_text_before_describe() {
    // Arrange
    let cycles = [0, 705]
        .into_iter()
        .flat_map(|oid| {
            [0, 1].map(move |format| {
                fixture::execute_cycle(
                    "SELECT $1 AS value WHERE FALSE",
                    Some((oid, 0, Some(b"true"))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-empty-text", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (format, frames) in [0, 1, 0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[25]);
        fixture::assert_single_column_descriptors(&frames, (25, -1, -1), format, true);
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_command(&frames, b"SELECT 0\0");
    }
}
