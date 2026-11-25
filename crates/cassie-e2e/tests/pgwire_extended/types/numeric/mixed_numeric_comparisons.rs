//! Known OIDs retain their wire identity across selected operator coercion.
use super::support_mixed_numeric::{boolean_row, projection};
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

const EQUAL: [Option<bool>; 8] = [
    Some(true),
    Some(false),
    Some(false),
    Some(true),
    Some(false),
    Some(true),
    Some(true),
    Some(false),
];

struct Case {
    oids: [i32; 2],
    bodies: [Option<Vec<u8>>; 2],
    input_format: i16,
    output_format: i16,
    expected: [Option<bool>; 8],
}

#[test]
fn should_preserve_known_inputs_across_mixed_comparison_domains() {
    // Arrange
    let numeric = [
        (21, 32_767_i64, "32767", 32_767.0_f64),
        (23, 2_147_483_647, "2147483647", 2_147_483_647.0),
        (
            20,
            9_007_199_254_740_993,
            "9007199254740992",
            9_007_199_254_740_992.0,
        ),
        (
            20,
            i64::MAX,
            "9223372036854775808",
            9_223_372_036_854_775_808.0,
        ),
        (
            20,
            i64::MIN,
            "-9223372036854775808",
            -9_223_372_036_854_775_808.0,
        ),
        (20, 0, "-0.0", -0.0),
    ];
    let mut cases = Vec::new();
    for (oid, integer, float_text, float) in numeric {
        for input_format in [0, 1] {
            let bodies = if input_format == 0 {
                [
                    Some(integer.to_string().into_bytes()),
                    Some(float_text.as_bytes().to_vec()),
                ]
            } else {
                [
                    Some(integer_body(oid, integer)),
                    Some(float.to_be_bytes().to_vec()),
                ]
            };
            for output_format in [0, 1] {
                for reverse in [false, true] {
                    let mut case = Case {
                        oids: [oid, 701],
                        bodies: bodies.clone(),
                        input_format,
                        output_format,
                        expected: EQUAL,
                    };
                    if reverse {
                        case.oids.reverse();
                        case.bodies.reverse();
                    }
                    cases.push(case);
                }
            }
        }
    }

    // Act
    let results = execute_cases("mixed-numeric-known-wire", &cases);

    // Assert
    assert_cases(&cases, &results);
}

#[test]
fn should_preserve_admitted_nonfinite_mixed_comparisons() {
    // Arrange
    let less = [
        Some(false),
        Some(true),
        Some(true),
        Some(true),
        Some(false),
        Some(false),
        Some(false),
        Some(true),
    ];
    let greater = [
        Some(false),
        Some(true),
        Some(false),
        Some(false),
        Some(true),
        Some(true),
        Some(false),
        Some(true),
    ];
    let mut cases = Vec::new();
    for (text, float, expected, reversed) in [
        ("Infinity", f64::INFINITY, less, greater),
        ("-Infinity", f64::NEG_INFINITY, greater, less),
        ("NaN", f64::NAN, less, greater),
        ("-NaN", -f64::NAN, less, greater),
    ] {
        for input_format in [0, 1] {
            let bodies = if input_format == 0 {
                [Some(b"7".to_vec()), Some(text.as_bytes().to_vec())]
            } else {
                [
                    Some(7_i64.to_be_bytes().to_vec()),
                    Some(float.to_be_bytes().to_vec()),
                ]
            };
            for output_format in [0, 1] {
                for reverse in [false, true] {
                    let mut case = Case {
                        oids: [20, 701],
                        bodies: bodies.clone(),
                        input_format,
                        output_format,
                        expected: if reverse { reversed } else { expected },
                    };
                    if reverse {
                        case.oids.reverse();
                        case.bodies.reverse();
                    }
                    cases.push(case);
                }
            }
        }
    }

    // Act
    let results = execute_cases("mixed-numeric-nonfinite-wire", &cases);

    // Assert
    assert_cases(&cases, &results);
}

#[test]
fn should_preserve_typed_null_input_identities_at_mixed_comparisons() {
    // Arrange
    let mut cases = Vec::new();
    for input_format in [0, 1] {
        let integer = if input_format == 0 {
            b"7".to_vec()
        } else {
            7_i64.to_be_bytes().to_vec()
        };
        let float = if input_format == 0 {
            b"7".to_vec()
        } else {
            7.0_f64.to_be_bytes().to_vec()
        };
        for output_format in [0, 1] {
            for (bodies, same) in [
                ([None, Some(float.clone())], false),
                ([Some(integer.clone()), None], false),
                ([None, None], true),
            ] {
                for reverse in [false, true] {
                    let mut case = Case {
                        oids: [20, 701],
                        bodies: bodies.clone(),
                        input_format,
                        output_format,
                        expected: [None, None, None, None, None, None, Some(same), Some(!same)],
                    };
                    if reverse {
                        case.oids.reverse();
                        case.bodies.reverse();
                    }
                    cases.push(case);
                }
            }
        }
    }

    // Act
    let results = execute_cases("mixed-numeric-null-wire", &cases);

    // Assert
    assert_cases(&cases, &results);
}

fn integer_body(oid: i32, value: i64) -> Vec<u8> {
    match oid {
        21 => i16::try_from(value)
            .expect("SMALLINT fixture")
            .to_be_bytes()
            .to_vec(),
        23 => i32::try_from(value)
            .expect("INT fixture")
            .to_be_bytes()
            .to_vec(),
        20 => value.to_be_bytes().to_vec(),
        _ => panic!("unexpected integer fixture OID"),
    }
}

fn execute_cases(label: &str, cases: &[Case]) -> Vec<fixture::Frames> {
    let sql = format!("SELECT {}", projection("$1", "$2"));
    let mut cycles = Vec::new();
    for case in cases {
        let parameters = case.bodies.iter().map(Option::as_deref).collect::<Vec<_>>();
        let cycle = vec![
            wire::parse_frame_with_types("", &sql, &case.oids),
            wire::describe_statement_frame(""),
            wire::bind_frame_with_formats(
                "",
                "",
                &[case.input_format; 2],
                &parameters,
                &[case.output_format],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ];
        // Repeat the exact statement and inputs across the warm result/plan path.
        cycles.push(cycle.clone());
        cycles.push(cycle);
        cycles.push(fixture::execute_cycle(
            "SELECT 1 AS recovery_marker",
            None,
            0,
            false,
        ));
    }
    fixture::run_cycles(label, &[], cycles)
}

fn assert_cases(cases: &[Case], results: &[fixture::Frames]) {
    assert_eq!(results.len(), cases.len() * 3);
    for (case, triple) in cases.iter().zip(results.as_chunks::<3>().0) {
        for frames in &triple[..2] {
            fixture::assert_success(frames);
            assert_eq!(
                frames.iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
                b"1tT2TDCZ"
            );
            let parameters = frames
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("parameter descriptor");
            assert_eq!(wire::parse_parameter_description(&parameters.1), case.oids);
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .collect::<Vec<_>>();
            assert_eq!(descriptions.len(), 2);
            for (index, (_, body)) in descriptions.into_iter().enumerate() {
                let fields = wire::parse_row_description(body);
                assert_eq!(fields.len(), 8);
                for (field, name) in
                    fields
                        .iter()
                        .zip(["eq", "ne", "lt", "le", "gt", "ge", "same", "different"])
                {
                    assert_eq!(field.name, name);
                    assert_eq!(
                        (
                            field.table_oid,
                            field.attr_num,
                            field.type_oid,
                            field.type_size,
                            field.type_mod
                        ),
                        (0, 0, 16, 1, -1)
                    );
                    assert_eq!(
                        field.format_code,
                        if index == 0 { 0 } else { case.output_format }
                    );
                }
            }
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![expected_payload(case)]
            );
            assert_eq!(
                frames
                    .iter()
                    .find(|(tag, _)| *tag == b'C')
                    .expect("command tag")
                    .1,
                b"SELECT 1\0"
            );
        }
        fixture::assert_success(&triple[2]);
        assert_eq!(
            wire::data_rows(&triple[2]),
            vec![vec![Some("1".to_owned())]]
        );
    }
}

fn expected_payload(case: &Case) -> Vec<u8> {
    let mut body = 8_i16.to_be_bytes().to_vec();
    for value in boolean_row(case.expected) {
        match value {
            Value::Null => body.extend_from_slice(&(-1_i32).to_be_bytes()),
            Value::Bool(value) => {
                body.extend_from_slice(&1_i32.to_be_bytes());
                body.push(if case.output_format == 1 {
                    u8::from(value)
                } else if value {
                    b't'
                } else {
                    b'f'
                });
            }
            _ => panic!("only retained Boolean/NULL fixtures"),
        }
    }
    body
}
