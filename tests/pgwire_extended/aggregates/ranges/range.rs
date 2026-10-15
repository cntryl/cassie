//! PostgreSQL 18 range and multirange wire identities tracked by #905.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../../types/ranges/discrete_integer_extrema.rs"]
mod discrete_integer_extrema;

fn bytes_from_hex(value: &str) -> Vec<u8> {
    let (pairs, _) = value.as_bytes().as_chunks::<2>();
    pairs
        .iter()
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
fn should_report_range_aggregate_result_oids() {
    // Arrange
    let sql = "SELECT range_agg(span) AS all_spans, range_intersect_agg(span) AS common_span FROM (VALUES (int4range(1, 3, '[)'))) AS inputs(span)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-905-range-aggregate-result-oids", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("range aggregate RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![4_451, 3_904]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("{[1,3)}".to_owned()), Some("[1,3)".to_owned())]]
    );
}

#[test]
fn should_report_multirange_aggregate_result_oids() {
    // Arrange
    let sql = "SELECT range_agg(span) AS merged, range_intersect_agg(span) AS intersection FROM (VALUES ('{[1,5),[9,11)}'::int4multirange), ('{[3,7),[9,12)}'::int4multirange)) AS inputs(span)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-905-multirange-aggregate-result-oids", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("multirange aggregate RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(4_451, -1, -1), (4_451, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("{[1,7),[9,12)}".to_owned()),
            Some("{[3,5),[9,11)}".to_owned()),
        ]]
    );
}

include!("../../types/ranges/range.rs");
