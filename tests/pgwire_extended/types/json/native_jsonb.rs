use super::{fixture, wire};

#[test]
fn should_preserve_document_cast_identity_given_json_types_when_values_cross_the_type_boundary() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"b\":1,\"a\":2}'::JSONB)::JSON, ('{\"b\":1,\"a\":2}'::JSON)::JSONB",
        None,
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("native-jsonb-cross-casts", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("document descriptors");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [114, 3802]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{\"a\": 2, \"b\": 1}".into()),
            Some("{\"a\": 2, \"b\": 1}".into())
        ]]
    );
}

#[test]
fn should_roundtrip_native_jsonb_given_binary_parameters_when_binary_results_are_requested() {
    // Arrange
    let input = b"\x01{\"n\":123456789012345678901234567890.125,\"s\":1.00}";
    let cycle = fixture::execute_cycle("SELECT $1::JSONB", Some((3802, 1, Some(input))), 1, true);

    // Act
    let batches = fixture::run_cycles("native-jsonb-binary", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (3802, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(
            b"\x01{\"n\": 123456789012345678901234567890.125, \"s\": 1.00}"
        )]
    );
}

#[test]
fn should_preserve_sql_null_given_native_jsonb_parameters_when_binary_results_are_requested() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT $1::JSONB", Some((3802, 1, None)), 1, true);

    // Act
    let batches = fixture::run_cycles("native-jsonb-sql-null", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (3802, -1, -1), 1, true);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None]]);
}

#[test]
fn should_classify_native_jsonb_binary_errors_given_invalid_input_when_the_connection_is_reused() {
    // Arrange
    let cases: &[(&[u8], &str)] = &[(b"\x02{}", "XX000"), (b"\x01bad", "22P02"), (b"", "08P01")];
    let cycles = cases
        .iter()
        .flat_map(|(input, _)| {
            [
                fixture::execute_cycle("SELECT $1::JSONB", Some((3802, 1, Some(input))), 1, true),
                fixture::execute_cycle("SELECT 7::INT", None, 0, true),
            ]
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("native-jsonb-binary-errors", &[], cycles);

    // Assert
    for ((_, code), frames) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&frames[0]).as_deref(), Some(*code));
        fixture::assert_success(&frames[1]);
        assert_eq!(wire::data_rows(&frames[1]), vec![vec![Some("7".into())]]);
    }
}

#[test]
fn should_preserve_native_jsonb_schema_given_stored_exact_documents_when_storage_restarts() {
    // Arrange
    let setup = [(
        "CREATE TABLE native_jsonb_restart (body JSONB, plain JSON)",
        Vec::new(),
    )];
    let input = b"\x01{\"n\":123456789012345678901234567890.125,\"s\":1.00}";
    let write = fixture::execute_cycle(
        "INSERT INTO native_jsonb_restart VALUES ($1, '{\"legacy\":true}'::JSON) RETURNING body",
        Some((3802, 1, Some(input))),
        1,
        true,
    );
    let read = fixture::execute_cycle(
        "SELECT body, plain FROM native_jsonb_restart",
        None,
        0,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "native-jsonb-restart",
        &setup,
        vec![vec![write, read.clone()], vec![read]],
    );

    // Assert
    fixture::assert_success(&stages[0][0]);
    assert_eq!(
        fixture::data_row_payloads(&stages[0][0]),
        vec![fixture::one_field_row(
            b"\x01{\"n\": 123456789012345678901234567890.125, \"s\": 1.00}"
        )]
    );
    for frames in [&stages[0][1], &stages[1][0]] {
        fixture::assert_success(frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("document descriptors");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            [3802, 114]
        );
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![
                Some("{\"n\": 123456789012345678901234567890.125, \"s\": 1.00}".into()),
                Some("{\"legacy\":true}".into())
            ]]
        );
    }
}

#[test]
fn should_reject_invalid_native_jsonb_given_text_parameters_when_the_connection_is_reused() {
    // Arrange
    let cases: &[(&[u8], &str)] = &[
        (br#"{"n":1e131073}"#, "22003"),
        (br#"{"s":"\u0000"}"#, "22P05"),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(input, _)| {
            [
                fixture::execute_cycle("SELECT $1::JSONB", Some((25, 0, Some(input))), 0, true),
                fixture::execute_cycle("SELECT 7::INT", None, 0, true),
            ]
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("native-jsonb-text-errors", &[], cycles);
    // Assert
    for ((_, code), frames) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&frames[0]).as_deref(), Some(*code));
        assert_eq!(
            wire::data_rows(&frames[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        fixture::assert_success(&frames[1]);
        assert_eq!(wire::data_rows(&frames[1]), vec![vec![Some("7".into())]]);
    }
}
