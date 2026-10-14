//! Direct PostgreSQL array function contracts not covered by the scalar-array matrix.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;
#[test]
fn should_report_two_dimensions_for_nested_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"{{1,2},{3,4}}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nested", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_report_one_dimension_for_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nondefault-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_prepend_elements_while_preserving_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_PREPEND(0, $1::INT[])::TEXT",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-prepend-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={0,7,NULL,9}".to_owned())]]
    );
}

#[test]
fn should_concatenate_arrays_while_preserving_postgresql_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_CAT($1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats(
            "",
            "",
            &[0, 0],
            &[Some(b"[0:1]={1,2}"), Some(b"[4:5]={3,4}")],
            &[],
        ),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-cat-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={1,2,3,4}".to_owned())]]
    );
}

#[test]
fn should_fill_multidimensional_arrays_from_explicit_lower_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_FILL(7, $1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats("", "", &[0, 0], &[Some(b"{2,3}"), Some(b"{0,-1}")], &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-fill-lower-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:1][-1:1]={{7,7,7},{7,7,7}}".to_owned())]]
    );
}

#[test]
fn should_round_trip_multidimensional_nondefault_bounds_in_binary_format() {
    // Arrange
    let input = b"\0\0\0\x02\0\0\0\x01\0\0\0\x17\0\0\0\x02\0\0\0\0\0\0\0\x02\xff\xff\xff\xff\0\0\0\x04\0\0\0\x01\xff\xff\xff\xff\0\0\0\x04\0\0\0\x03\0\0\0\x04\0\0\0\x04";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| {
            fixture::execute_cycle(
                "SELECT $1::INT4[] AS value",
                Some((1_007, 1, Some(input))),
                format,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-multidimensional-binary-bounds",
        &[],
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (1_007, -1, -1), format, true);
        if format == 0 {
            assert_eq!(
                wire::data_rows(&frames),
                vec![vec![Some("[0:1][-1:0]={{1,NULL},{3,4}}".to_owned())]]
            );
        } else {
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![fixture::one_field_row(input)]
            );
        }
    }
}

#[test]
fn should_preserve_three_array_null_states_through_wire_formats() {
    // Arrange
    struct ScalarCase {
        sql: &'static str,
        oid: i32,
        size: i16,
        text: Option<&'static [u8]>,
        binary: Option<&'static [u8]>,
    }

    let empty_array_binary = b"\0\0\0\0\0\0\0\0\0\0\0\x17";
    let null_element_array_binary =
        b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x01\0\0\0\x01\xff\xff\xff\xff";
    let null_field_row = vec![0, 1, 255, 255, 255, 255];
    let cases = [
        ScalarCase {
            sql: "SELECT NULL::INT4[]",
            oid: 1_007,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarCase {
            sql: "SELECT ARRAY[]::INT4[]",
            oid: 1_007,
            size: -1,
            text: Some(b"{}"),
            binary: Some(empty_array_binary),
        },
        ScalarCase {
            sql: "SELECT ARRAY[NULL]::INT4[]",
            oid: 1_007,
            size: -1,
            text: Some(b"{NULL}"),
            binary: Some(null_element_array_binary),
        },
        ScalarCase {
            sql: "SELECT array_dims(NULL::INT4[])",
            oid: 25,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarCase {
            sql: "SELECT cardinality(NULL::INT4[])",
            oid: 23,
            size: 4,
            text: None,
            binary: None,
        },
        ScalarCase {
            sql: "SELECT array_dims(ARRAY[]::INT4[])",
            oid: 25,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarCase {
            sql: "SELECT array_length(ARRAY[]::INT4[], 1)",
            oid: 23,
            size: 4,
            text: None,
            binary: None,
        },
        ScalarCase {
            sql: "SELECT cardinality(ARRAY[]::INT4[])",
            oid: 23,
            size: 4,
            text: Some(b"0"),
            binary: Some(b"\0\0\0\0"),
        },
        ScalarCase {
            sql: "SELECT array_position(ARRAY[NULL]::INT4[], NULL)",
            oid: 23,
            size: 4,
            text: Some(b"1"),
            binary: Some(b"\0\0\0\x01"),
        },
        ScalarCase {
            sql: "SELECT ARRAY[NULL]::INT4[] IS NULL",
            oid: 16,
            size: 1,
            text: Some(b"f"),
            binary: Some(b"\0"),
        },
    ];
    let expectations = cases
        .iter()
        .flat_map(|case| {
            [0, 1].into_iter().map(move |format| {
                (
                    case.oid,
                    case.size,
                    format,
                    if format == 0 { case.text } else { case.binary },
                )
            })
        })
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .flat_map(|case| {
            [0, 1]
                .into_iter()
                .map(move |format| fixture::execute_cycle(case.sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-array-null-empty-element-wire", &[], cycles);

    // Assert
    assert_eq!(batches.len(), expectations.len());
    for ((oid, size, format, expected), frames) in expectations.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (oid, size, -1), format, true);
        let expected_row = expected.map_or_else(
            || null_field_row.clone(),
            |value| fixture::one_field_row(value),
        );
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected_row]);
    }
}

#[test]
fn should_correlate_series_ordinality_to_each_outer_row() {
    // Arrange
    let sql = "SELECT source.id, generated.value, generated.ordinality FROM (VALUES (1, 2, 4), (2, 5, 6), (3, 3, 2)) AS source(id, first_value, last_value) LEFT JOIN LATERAL generate_series(source.first_value, source.last_value) WITH ORDINALITY AS generated(value, ordinality) ON TRUE ORDER BY source.id, generated.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-808-correlated-generate-series-ordinality",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("correlated generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("2".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("3".to_owned()),
                Some("2".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("4".to_owned()),
                Some("3".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("5".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("6".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("3".to_owned()), None, None],
        ]
    );
}

#[test]
fn should_stop_returning_generate_series_rows_at_limit() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT value FROM generate_series(1, 10000) AS series(value) LIMIT 2",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-808-generate-series-limit", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
}
