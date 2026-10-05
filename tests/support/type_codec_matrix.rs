//! Finite current-type codec fixtures; packet literals never call Cassie codecs.

use crate::support_type_metadata_contract as wire_fixture;
use cassie::types::Value;

#[path = "type_codec_matrix/array_cases.rs"]
mod array_cases;
#[path = "type_codec_matrix/scalar_cases.rs"]
mod scalar_cases;

pub use array_cases::ARRAY_CASES;
pub use scalar_cases::SCALAR_CASES;

pub struct CodecCase {
    pub name: &'static str,
    pub sql_type: &'static str,
    pub oid: i32,
    pub typlen: i16,
    pub typmod: i32,
    pub input_text: &'static [u8],
    pub output_text: &'static [u8],
    pub stored_output_text: &'static [u8],
    pub input_binary: &'static [u8],
    pub output_binary: &'static [u8],
    pub empty_binary: Option<&'static [u8]>,
}

pub struct Expected {
    case: &'static CodecCase,
    format: i16,
    has_row: bool,
    field: Option<&'static [u8]>,
}

pub fn setup_queries(prefix: &str, cases: &[CodecCase]) -> Vec<String> {
    cases
        .iter()
        .map(|case| {
            format!(
                "CREATE TABLE {prefix}_{} (k INT, v {})",
                case.name, case.sql_type
            )
        })
        .collect()
}

pub fn setup_refs(statements: &[String]) -> Vec<(&str, Vec<Value>)> {
    statements
        .iter()
        .map(|statement| (statement.as_str(), Vec::new()))
        .collect()
}

pub fn populated_cycles(
    prefix: &str,
    cases: &'static [CodecCase],
) -> (Vec<wire_fixture::Cycle>, Vec<Expected>) {
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in cases {
        let mut key = 1;
        for (input_format, body) in [(0, case.input_text), (1, case.input_binary)] {
            for format in [0, 1] {
                let sql = format!(
                    "INSERT INTO {prefix}_{} (k, v) VALUES ({key}, $1) RETURNING v",
                    case.name
                );
                cycles.push(wire_fixture::execute_cycle(
                    &sql,
                    Some((case.oid, input_format, Some(body))),
                    format,
                    true,
                ));
                expected.push(value_expectation(case, format, case.output_text));
                key += 1;
            }
        }
        // Re-read the first actual stored value; RETURNING alone does not
        // prove the row codec preserved its declared family.
        for format in [0, 1] {
            let sql = format!("SELECT v FROM {prefix}_{} WHERE k = 1", case.name);
            cycles.push(wire_fixture::execute_cycle(&sql, None, format, true));
            expected.push(value_expectation(case, format, case.stored_output_text));
        }
    }
    (cycles, expected)
}

fn value_expectation(case: &'static CodecCase, format: i16, text: &'static [u8]) -> Expected {
    Expected {
        case,
        format,
        has_row: true,
        field: Some(if format == 0 {
            text
        } else {
            case.output_binary
        }),
    }
}

pub fn descriptor_cycles(
    prefix: &str,
    cases: &'static [CodecCase],
    has_row: bool,
) -> (Vec<wire_fixture::Cycle>, Vec<Expected>) {
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in cases {
        for format in [0, 1] {
            let sql = if has_row {
                format!("SELECT CAST(NULL AS {}) AS v", case.sql_type)
            } else {
                format!("SELECT v FROM {prefix}_{}", case.name)
            };
            cycles.push(wire_fixture::execute_cycle(&sql, None, format, true));
            expected.push(Expected {
                case,
                format,
                has_row,
                field: None,
            });
        }
    }
    (cycles, expected)
}

pub fn empty_array_cycles(prefix: &str) -> (Vec<wire_fixture::Cycle>, Vec<Expected>) {
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for case in &ARRAY_CASES {
        let empty_binary = case.empty_binary.expect("array family empty literal");
        let mut key = 1;
        for (input_format, body) in [(0, b"{}".as_slice()), (1, empty_binary)] {
            for format in [0, 1] {
                let sql = format!(
                    "INSERT INTO {prefix}_{} (k, v) VALUES ({key}, $1) RETURNING v",
                    case.name
                );
                cycles.push(wire_fixture::execute_cycle(
                    &sql,
                    Some((case.oid, input_format, Some(body))),
                    format,
                    true,
                ));
                expected.push(Expected {
                    case,
                    format,
                    has_row: true,
                    field: Some(if format == 0 { b"[]" } else { empty_binary }),
                });
                key += 1;
            }
        }
        for format in [0, 1] {
            let sql = format!("SELECT v FROM {prefix}_{} WHERE k = 1", case.name);
            cycles.push(wire_fixture::execute_cycle(&sql, None, format, true));
            expected.push(Expected {
                case,
                format,
                has_row: true,
                field: Some(if format == 0 { b"[]" } else { empty_binary }),
            });
        }
    }
    (cycles, expected)
}

pub fn assert_batches(frames: &[wire_fixture::Frames], expected: &[Expected]) {
    assert_eq!(frames.len(), expected.len());
    for (index, (frames, expected)) in frames.iter().zip(expected).enumerate() {
        assert_eq!(
            crate::support_pgwire::error_code(frames),
            None,
            "cycle {index}: {} format {}, frames {frames:?}",
            expected.case.sql_type,
            expected.format
        );
        wire_fixture::assert_success(frames);
        wire_fixture::assert_single_column_descriptors(
            frames,
            (
                expected.case.oid,
                expected.case.typlen,
                expected.case.typmod,
            ),
            expected.format,
            true,
        );
        let rows = if expected.has_row {
            vec![expected.field.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                wire_fixture::one_field_row,
            )]
        } else {
            Vec::new()
        };
        assert_eq!(
            wire_fixture::data_row_payloads(frames),
            rows,
            "{} format {}",
            expected.case.sql_type,
            expected.format
        );
    }
}
