//! Native table-source parameters, positional descriptors and recovery.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn assert_columns(frames: &fixture::Frames, format: i16, expected: &[(&str, i32)]) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for (index, (_, payload)) in descriptions.into_iter().enumerate() {
        let columns = wire::parse_row_description(payload);
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.name.as_str(), column.type_oid))
                .collect::<Vec<_>>(),
            expected
        );
        for column in &columns {
            assert_eq!(
                column.type_size,
                match column.type_oid {
                    23 => 4,
                    20 => 8,
                    _ => -1,
                }
            );
        }
        assert!(columns.iter().all(|column| column.type_mod == -1
            && column.format_code == if index == 0 { 0 } else { format }));
    }
}

#[test]
fn should_emit_native_unnest_rows_from_bound_array_parameters() {
    // Arrange
    let sql = "SELECT item, position FROM unnest($1::INT[]) WITH ORDINALITY AS s(item,position)";
    let inputs = [Some("[-2:0]={1,NULL,3}"), Some("{}"), None];
    let cycles = inputs
        .iter()
        .map(|input| fixture::execute_cycle(sql, Some((0, 0, input.map(str::as_bytes))), 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-unnest-parameters", &[], cycles);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
        assert_columns(frames, 0, &[("item", 23), ("position", 20)]);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![1_007]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("3".to_owned()), Some("3".to_owned())],
        ]
    );
    assert_eq!(wire::data_rows(&batches[1]), [] as [Vec<Option<String>>; 0]);
    assert_eq!(wire::data_rows(&batches[2]), [] as [Vec<Option<String>>; 0]);
}

fn native_int4_array() -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in [1_i32, 1, 23, 3, -2, 4, 1, -1, 4, 3] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes
}

fn row(fields: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut bytes = i16::try_from(fields.len())
        .expect("field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        let length = field.as_ref().map_or(-1, |value| {
            i32::try_from(value.len()).expect("field length")
        });
        bytes.extend_from_slice(&length.to_be_bytes());
        if let Some(value) = field {
            bytes.extend_from_slice(value);
        }
    }
    bytes
}

#[test]
fn should_encode_unnest_elements_using_native_scalar_binary_widths() {
    // Arrange
    let sql = "SELECT item, position FROM unnest($1) WITH ORDINALITY AS s(item,position)";
    let input = native_int4_array();
    let cycle = fixture::execute_cycle(sql, Some((1_007, 1, Some(&input))), 1, true);

    // Act
    let batches = fixture::run_cycles("issue-808-unnest-binary", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_columns(frames, 1, &[("item", 23), ("position", 20)]);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            row(&[
                Some(1_i32.to_be_bytes().to_vec()),
                Some(1_i64.to_be_bytes().to_vec())
            ]),
            row(&[None, Some(2_i64.to_be_bytes().to_vec())]),
            row(&[
                Some(3_i32.to_be_bytes().to_vec()),
                Some(3_i64.to_be_bytes().to_vec())
            ]),
        ]
    );
}

#[test]
fn should_terminate_invalid_unnest_requests_at_their_native_diagnostic() {
    // Arrange
    let cases = [
        ("SELECT * FROM unnest(NULL)", "42725"),
        ("SELECT * FROM unnest($1)", "42725"),
        ("SELECT * FROM unnest(1)", "42883"),
        ("SELECT unnest FROM unnest(ARRAY[1],ARRAY[2])", "42702"),
        ("SELECT * FROM unnest(ARRAY[1]) s(a,b)", "42P10"),
    ];
    let mut cycles = Vec::new();
    for (sql, _) in cases {
        cycles.push(fixture::execute_cycle(sql, None, 0, true));
        cycles.push(fixture::execute_cycle("SELECT 7", None, 0, true));
    }

    // Act
    let batches = fixture::run_cycles("issue-808-unnest-error-recovery", &[], cycles);

    // Assert
    for ((_, state), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0.iter()) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some(state));
        assert_eq!(
            pair[0]
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("7".to_owned())]]);
    }
}

#[test]
fn should_bind_computed_array_sources_without_losing_native_parameter_identity() {
    // Arrange
    let cases = [
        (
            "SELECT item FROM unnest(pg_catalog.array_fill($1,ARRAY[2])) s(item)",
            23,
            "7",
            vec![Some("7"), Some("7")],
        ),
        (
            "SELECT item FROM unnest(ARRAY[$1,NULL]) s(item)",
            23,
            "7",
            vec![Some("7"), None],
        ),
        (
            "SELECT item FROM unnest(array_append($1,2)) s(item)",
            1007,
            "{1}",
            vec![Some("1"), Some("2")],
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, oid, input, _)| {
            fixture::execute_cycle(sql, Some((*oid, 0, Some(input.as_bytes()))), 0, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-computed-array-parameter", &[], cycles);

    // Assert
    for ((_, input_oid, _, expected), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![input_oid]
        );
        assert_eq!(
            wire::data_rows(frames),
            expected
                .into_iter()
                .map(|value| vec![value.map(str::to_owned)])
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn should_drop_input_element_modifiers_from_unnest_result_descriptors() {
    // Arrange
    let cases = [
        (
            "SELECT * FROM unnest(ARRAY['a'::VARCHAR(3)])",
            1_043,
            -1,
            "a",
        ),
        (
            "SELECT * FROM unnest(ARRAY['a'::CHAR(3)])",
            1_042,
            -1,
            "a  ",
        ),
        (
            "SELECT * FROM unnest(ARRAY[1.20::NUMERIC(4,2)])",
            1_700,
            -1,
            "1.20",
        ),
        (
            "SELECT * FROM unnest(ARRAY['12:34:56.1234'::TIME(3)])",
            1_083,
            8,
            "12:34:56.123",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-unnest-input-modifiers", &[], cycles);

    // Assert
    for ((sql, oid, width, value), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, width, -1), 0, true);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(value.to_owned())]],
            "{sql}"
        );
    }
}

#[test]
fn should_retain_parameter_identity_through_array_subquery_source_arguments() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT * FROM unnest(ARRAY(SELECT $1::INT))",
        Some((23, 0, Some(b"7"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-808-unnest-subquery-parameter", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    let parameters = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}
