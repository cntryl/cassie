//! PostgreSQL 18 range and multirange wire identities tracked by #905.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn bytes_from_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("hexadecimal bytes are UTF-8");
            u8::from_str_radix(digits, 16).expect("hexadecimal byte")
        })
        .collect()
}

fn binary_row(fields: &[&[u8]]) -> Vec<u8> {
    let mut row = i16::try_from(fields.len())
        .expect("field count fits a PostgreSQL row")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        row.extend_from_slice(
            &i32::try_from(field.len())
                .expect("field size fits a PostgreSQL data row")
                .to_be_bytes(),
        );
        row.extend_from_slice(field);
    }
    row
}

const BUILTIN_RANGE_OIDS: [i32; 12] = [
    3_904, 3_926, 3_906, 3_912, 3_908, 3_910, 4_451, 4_536, 4_532, 4_535, 4_533, 4_534,
];

const BUILTIN_RANGE_TYPE_NAMES: [&str; 12] = [
    "int4range",
    "int8range",
    "numrange",
    "daterange",
    "tsrange",
    "tstzrange",
    "int4multirange",
    "int8multirange",
    "nummultirange",
    "datemultirange",
    "tsmultirange",
    "tstzmultirange",
];

const BUILTIN_RANGE_TEXT_INPUTS: [&[u8]; 12] = [
    b"[1,4]",
    b"[1,4]",
    b"[1.20,2.40)",
    b"[2020-01-01,2020-01-03]",
    b"[2020-01-01 00:00:00,2020-01-03 00:00:00)",
    b"[2020-01-01 00:00:00+00,2020-01-03 00:00:00+00)",
    b"{[1,3),[3,5)}",
    b"{[1,3),[3,5)}",
    b"{[1.20,2.40),[2.40,3.60)}",
    b"{[2020-01-01,2020-01-02),[2020-01-02,2020-01-03)}",
    b"{[2020-01-01 00:00:00,2020-01-02 00:00:00),[2020-01-02 00:00:00,2020-01-03 00:00:00)}",
    b"{[2020-01-01 00:00:00+00,2020-01-02 00:00:00+00),[2020-01-02 00:00:00+00,2020-01-03 00:00:00+00)}",
];

const BUILTIN_RANGE_BINARY_INPUT_HEX: [&str; 12] = [
    "0200000004000000010000000400000005",
    "02000000080000000000000001000000080000000000000005",
    "020000000a000100000000000000010000000a00010000000000000005",
    "020000000400001c890000000400001c8b",
    "020000000800023e0786c260000000000800023e2fc2712000",
    "020000000800023e0786c260000000000800023e2fc2712000",
    "00000001000000110200000004000000010000000400000005",
    "000000010000001902000000080000000000000001000000080000000000000005",
    "000000010000001d020000000a000100000000000000010000000a00010000000000000005",
    "0000000100000011020000000400001c890000000400001c8b",
    "0000000100000019020000000800023e0786c260000000000800023e2fc2712000",
    "0000000100000019020000000800023e0786c260000000000800023e2fc2712000",
];

const BUILTIN_RANGE_TEXT_RESULTS: [Option<&str>; 12] = [
    Some("[1,5)"),
    Some("[1,5)"),
    Some("[1,5)"),
    Some("[2020-01-01,2020-01-03)"),
    Some("[\"2020-01-01 00:00:00\",\"2020-01-03 00:00:00\")"),
    None,
    Some("{[1,5)}"),
    Some("{[1,5)}"),
    Some("{[1,5)}"),
    Some("{[2020-01-01,2020-01-03)}"),
    Some("{[\"2020-01-01 00:00:00\",\"2020-01-03 00:00:00\")}"),
    None,
];

const BUILTIN_RANGE_TEXT_INPUT_RESULTS: [Option<&str>; 12] = [
    Some("[1,5)"),
    Some("[1,5)"),
    Some("[1.20,2.40)"),
    Some("[2020-01-01,2020-01-04)"),
    Some("[\"2020-01-01 00:00:00\",\"2020-01-03 00:00:00\")"),
    None,
    Some("{[1,5)}"),
    Some("{[1,5)}"),
    Some("{[1.20,3.60)}"),
    Some("{[2020-01-01,2020-01-03)}"),
    Some("{[\"2020-01-01 00:00:00\",\"2020-01-03 00:00:00\")}"),
    None,
];

fn range_parameter_sql() -> String {
    let projections = (1..=BUILTIN_RANGE_OIDS.len())
        .zip(BUILTIN_RANGE_TYPE_NAMES)
        .map(|(index, type_name)| format!("${index}::{type_name} AS range_value_{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {projections}")
}

fn range_parameter_steps(
    statement: &str,
    parameter_format: i16,
    parameters: &[Option<&[u8]>],
) -> Vec<Step> {
    let portal = format!("{statement}_portal");
    let sql = range_parameter_sql();
    vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, &sql, &[0; 12])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(&portal, statement, &[parameter_format], parameters, &[]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]),
    ]
}

fn assert_range_parameter_results(
    batches: &[Vec<(u8, Vec<u8>)>],
    expected_text: &[Option<&str>; 12],
) {
    assert_eq!(batches.len(), 3);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));

    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("range ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        BUILTIN_RANGE_OIDS
    );

    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("range parameter result RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), BUILTIN_RANGE_OIDS.len());
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        BUILTIN_RANGE_OIDS
    );
    assert!(columns
        .iter()
        .all(|column| column.type_size == -1 && column.type_mod == -1));
    assert!(columns.iter().all(|column| column.format_code == 0));
    let rows = wire::data_rows(result);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), BUILTIN_RANGE_OIDS.len());
    for (actual, expected) in rows[0].iter().zip(expected_text) {
        if let Some(expected) = expected {
            assert_eq!(actual.as_deref(), Some(*expected));
        } else {
            assert!(actual.is_some(), "timestamptz range should be non-NULL");
        }
    }
}

#[test]
fn should_describe_null_int4range_with_postgresql_type_identity() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::int4range AS value",
    )])];

    // Act
    let batches = run_script("postgresql18-int4range-null-identity", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("int4range RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (3_904, -1, -1)
    );
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
}

#[test]
fn should_describe_null_int4multirange_with_postgresql_type_identity() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::int4multirange AS value",
    )])];

    // Act
    let batches = run_script("postgresql18-int4multirange-null-identity", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("int4multirange RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (4_451, -1, -1)
    );
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
}

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
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[1,5)".to_owned()),
            Some("[1.20,2.40)".to_owned()),
            Some("empty".to_owned()),
            Some("[2020-01-01,2020-01-04)".to_owned()),
            Some("{[1,5)}".to_owned()),
        ]]
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
fn should_send_every_builtin_range_family_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let cases = [
        (3_904, "0200000004000000010000000400000005"),
        (3_926, "02000000080000000000000001000000080000000000000005"),
        (
            3_906,
            "020000000a000100000000000000010000000a00010000000000000005",
        ),
        (3_912, "020000000400001c890000000400001c8b"),
        (3_908, "020000000800023e0786c260000000000800023e2fc2712000"),
        (3_910, "020000000800023e0786c260000000000800023e2fc2712000"),
        (4_451, "00000001000000110200000004000000010000000400000005"),
        (
            4_536,
            "000000010000001902000000080000000000000001000000080000000000000005",
        ),
        (
            4_532,
            "000000010000001d020000000a000100000000000000010000000a00010000000000000005",
        ),
        (4_535, "0000000100000011020000000400001c890000000400001c8b"),
        (
            4_533,
            "0000000100000019020000000800023e0786c260000000000800023e2fc2712000",
        ),
        (
            4_534,
            "0000000100000019020000000800023e0786c260000000000800023e2fc2712000",
        ),
    ];
    let sql = "SELECT '[1,5)'::int4range AS int4_range, '[1,5)'::int8range AS int8_range, '[1,5)'::numrange AS numeric_range, '[2020-01-01,2020-01-03)'::daterange AS date_range, '[2020-01-01 00:00:00,2020-01-03 00:00:00)'::tsrange AS timestamp_range, '[2020-01-01 00:00:00+00,2020-01-03 00:00:00+00)'::tstzrange AS timestamptz_range, '{[1,5)}'::int4multirange AS int4_multirange, '{[1,5)}'::int8multirange AS int8_multirange, '{[1,5)}'::nummultirange AS numeric_multirange, '{[2020-01-01,2020-01-03)}'::datemultirange AS date_multirange, '{[2020-01-01 00:00:00,2020-01-03 00:00:00)}'::tsmultirange AS timestamp_multirange, '{[2020-01-01 00:00:00+00,2020-01-03 00:00:00+00)}'::tstzmultirange AS timestamptz_multirange";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("all_range_binary_types", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "all_range_binary_portal",
                "all_range_binary_types",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("all_range_binary_portal"),
            wire::execute_frame("all_range_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-905-all-range-binary-families", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("all range family RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        cases.iter().map(|(oid, _)| *oid).collect::<Vec<_>>()
    );
    assert!(columns
        .iter()
        .all(|column| column.type_size == -1 && column.type_mod == -1));
    assert!(columns.iter().all(|column| column.format_code == 1));

    let expected_fields = cases
        .iter()
        .map(|(_, hex)| bytes_from_hex(hex))
        .collect::<Vec<_>>();
    let expected_field_slices = expected_fields
        .iter()
        .map(Vec::as_slice)
        .collect::<Vec<_>>();
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![binary_row(&expected_field_slices)]
    );
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

#[test]
fn should_decode_text_parameters_for_every_builtin_range_family() {
    // Arrange
    let parameters = BUILTIN_RANGE_TEXT_INPUTS
        .iter()
        .map(|value| Some(*value))
        .collect::<Vec<_>>();
    let steps = range_parameter_steps("all_range_text_inputs", 0, &parameters);

    // Act
    let batches = run_script("issue-905-all-range-text-inputs", steps);

    // Assert
    assert_range_parameter_results(&batches, &BUILTIN_RANGE_TEXT_INPUT_RESULTS);
}

#[test]
fn should_decode_binary_parameters_for_every_builtin_range_family() {
    // Arrange
    let decoded_parameters = BUILTIN_RANGE_BINARY_INPUT_HEX
        .iter()
        .map(|value| bytes_from_hex(value))
        .collect::<Vec<_>>();
    let parameters = decoded_parameters
        .iter()
        .map(|value| Some(value.as_slice()))
        .collect::<Vec<_>>();
    let steps = range_parameter_steps("all_range_binary_inputs", 1, &parameters);

    // Act
    let batches = run_script("issue-905-all-range-binary-inputs", steps);

    // Assert
    assert_range_parameter_results(&batches, &BUILTIN_RANGE_TEXT_RESULTS);
}

#[test]
fn should_compare_timestamptz_range_values_by_instant() {
    // Arrange
    let sql = "SELECT '[2020-01-01 01:00:00+01,2020-01-03 01:00:00+01)'::tstzrange = '[2020-01-01 00:00:00+00,2020-01-03 00:00:00+00)'::tstzrange, '{[2020-01-01 01:00:00+01,2020-01-02 01:00:00+01),[2020-01-02 01:00:00+01,2020-01-03 01:00:00+01)}'::tstzmultirange = '{[2020-01-01 00:00:00+00,2020-01-03 00:00:00+00)}'::tstzmultirange";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-905-timestamptz-range-instant-values", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("timestamptz range comparison RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("t".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_report_malformed_range_text_with_invalid_text_representation_sqlstate() {
    // PostgreSQL 18.6 returns 22P02 for this malformed range literal. Cassie
    // currently stops earlier with 0A000 because int4range is unsupported, so
    // this acceptance expectation is masked and does not prove input parsing.
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 'not-a-range'::int4range",
    )])];

    // Act
    let batches = run_script("issue-905-malformed-range-text", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        Some("22P02".to_owned()),
        "wire errors: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
}
