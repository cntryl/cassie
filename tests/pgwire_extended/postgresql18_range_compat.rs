//! PostgreSQL 18 range and multirange wire identities tracked by #905.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_describe_builtin_range_families_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT '[1,4]'::int4range AS integer_range, '[1.20,2.40)'::numrange AS numeric_range, 'empty'::int8range AS empty_range, daterange('2020-01-01', '2020-01-03', '[]') AS date_range, '{[1,3),[3,5)}'::int4multirange AS integer_multirange";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-range-wire-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("range RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3_904, 3_906, 3_926, 3_912, 4_451]
    );
}

#[test]
fn should_send_range_families_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let range_binary = [
        0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x05,
    ];
    let multirange_binary = [
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x11, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "range_binary_statement",
            "SELECT '[1,5)'::int4range AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "range_binary_portal",
                "range_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("range_binary_portal"),
            wire::execute_frame("range_binary_portal"),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame(
            "multirange_binary_statement",
            "SELECT '{[1,3),[3,5)}'::int4multirange AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "multirange_binary_portal",
                "multirange_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("multirange_binary_portal"),
            wire::execute_frame("multirange_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-905-range-binary-output", steps);

    // Assert
    for ((frames, oid, expected), parse_complete) in [
        (&batches[1], 3_904, &range_binary[..]),
        (&batches[3], 4_451, &multirange_binary[..]),
    ]
    .into_iter()
    .zip([&batches[0], &batches[2]])
    {
        assert_eq!(wire::error_code(parse_complete), None);
        assert_eq!(parse_complete.first().map(|(tag, _)| *tag), Some(b'1'));
        assert_eq!(wire::error_code(frames), None);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("range binary RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!(
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ),
            (oid, -1, -1, 1)
        );
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_describe_timestamp_ranges_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT NULL::tsrange AS tsrange, NULL::tstzrange AS tstzrange, NULL::nummultirange AS nummultirange, NULL::tsmultirange AS tsmultirange, NULL::tstzmultirange AS tstzmultirange, NULL::datemultirange AS datemultirange, NULL::int8multirange AS int8multirange";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-905-range-multirange-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("timestamp range and multirange RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3_908, 3_910, 4_532, 4_533, 4_534, 4_535, 4_536]
    );
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None; 7]]);
}

#[test]
fn should_return_text_for_binary_int4range_parameters() {
    // Arrange
    let range_binary = [
        0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x05,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "range_binary_input_statement",
            "SELECT $1::int4range AS value",
            &[3_904],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "range_binary_input_portal",
                "range_binary_input_statement",
                &[1],
                &[Some(&range_binary)],
                &[0],
            ),
            wire::describe_portal_frame("range_binary_input_portal"),
            wire::execute_frame("range_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-905-range-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("range binary-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (3_904, -1, -1, 0)
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("[1,5)".to_owned())]]
    );
}

#[test]
fn should_decode_binary_int4multirange_parameters() {
    // Arrange
    let multirange_binary = [
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x11, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x05,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "int4multirange_binary_input",
            "SELECT $1::text AS value",
            &[4_451],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "int4multirange_binary_input_portal",
                "int4multirange_binary_input",
                &[1],
                &[Some(&multirange_binary)],
                &[0],
            ),
            wire::describe_portal_frame("int4multirange_binary_input_portal"),
            wire::execute_frame("int4multirange_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-905-int4multirange-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("int4multirange binary-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.format_code), (25, 0));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("{[1,5)}".to_owned())]]
    );
}
