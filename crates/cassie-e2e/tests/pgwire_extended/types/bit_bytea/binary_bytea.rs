//! PostgreSQL 18 bit-string and bytea wire contracts tracked by #816.

#[path = "bit_string_error_null_boundaries.rs"]
mod bit_string_error_null_boundaries;
#[path = "bit_string_oid_zero_binary_bind.rs"]
mod bit_string_oid_zero_binary_bind;
#[path = "bit_string_operations_wire.rs"]
mod bit_string_operations_wire;
#[path = "bytea_bits.rs"]
mod bytea_bits;
#[path = "computed_binary_operations.rs"]
mod computed_binary_operations;
#[path = "integer_bytea_casts.rs"]
mod integer_bytea_casts;
#[path = "pg_type_vectors.rs"]
mod pg_type_vectors;
#[path = "varbit_empty_boundary.rs"]
mod varbit_empty_boundary;
#[path = "varbit_ordering_boundaries.rs"]
mod varbit_ordering_boundaries;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn row_description(frames: &[(u8, Vec<u8>)]) -> Vec<wire::RowDescription> {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(payload)
}

fn data_row_bytes(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<Vec<u8>>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| {
            let field_count = i16::from_be_bytes([payload[0], payload[1]]);
            let mut cursor = 2;
            let mut fields = Vec::new();
            for _ in 0..field_count {
                let length = i32::from_be_bytes(
                    payload[cursor..cursor + 4]
                        .try_into()
                        .expect("DataRow field length"),
                );
                cursor += 4;
                assert!(length >= 0, "binary fixture fields should not be NULL");
                let end = cursor + usize::try_from(length).expect("field length fits usize");
                fields.push(payload[cursor..end].to_vec());
                cursor = end;
            }
            fields
        })
        .collect()
}

#[test]
fn should_describe_postgresql_binary_type_oids() {
    // Arrange
    let batches = run_script(
        "issue-816-binary-type-descriptors",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            r"SELECT B'101'::bit(3) AS fixed, B'00101'::varbit AS variable, '\x0102'::bytea AS payload",
        )])],
    );

    // Act
    let result = &batches[0];
    let fields = row_description(result);

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        fields
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![1560, 1562, 17]
    );
    assert_eq!(
        fields
            .iter()
            .map(|field| field.type_mod)
            .collect::<Vec<_>>(),
        vec![3, -1, -1]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("101".to_owned()),
            Some("00101".to_owned()),
            Some(r"\x0102".to_owned())
        ]]
    );
}

#[test]
fn should_bind_text_parameters_for_bit_varbit_bytea() {
    // Arrange
    let sql = "SELECT $1::bit(3) AS fixed, $2::varbit AS variable, $3::bytea AS payload";
    let batches = run_script(
        "issue-816-binary-text-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "binary_text_parameters",
                sql,
                &[1560, 1562, 17],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("binary_text_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "binary_text_portal",
                    "binary_text_parameters",
                    &["101", "00101", r"\x00ff"],
                ),
                wire::execute_frame("binary_text_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let result = &batches[2];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1560, 1562, 17]
    );
    assert_eq!(
        row_description(described)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![1560, 1562, 17]
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("101".to_owned()),
            Some("00101".to_owned()),
            Some(r"\x00ff".to_owned())
        ]]
    );
}

#[test]
fn should_report_invalid_bytea_text_bind_with_postgresql_sqlstate() {
    // Arrange
    let statement_cases = [
        ("bytea_odd_hex_input", "bytea_odd_hex_portal", r"\xabc"),
        ("bytea_non_hex_input", "bytea_non_hex_portal", r"\x0g"),
    ];
    let mut steps = Vec::new();
    for (statement, portal, value) in statement_cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1::BYTEA AS payload",
            &[17],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &[value]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42 AS recovered",
        )]));
    }

    // Act
    let batches = run_script("postgresql18-bytea-text-input-error-sqlstates", steps);

    // Assert
    assert_eq!(batches.len(), statement_cases.len() * 4);
    for (index, (statement, _, _)) in statement_cases.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(
            wire::error_code(&batches[offset]),
            None,
            "{statement} Parse"
        );
        let parameter_description = batches[offset + 1]
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("bytea ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameter_description.1),
            vec![17],
            "{statement} parameter OID"
        );
        let error_batch = &batches[offset + 2];
        assert_eq!(
            wire::error_code(error_batch).as_deref(),
            Some("22023"),
            "{statement} PostgreSQL 18.6 bytea input SQLSTATE"
        );
        assert!(
            !error_batch
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
            "{statement} must fail before returning a result"
        );
        assert_eq!(
            error_batch.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "{statement} should return ReadyForQuery after the Bind error"
        );
        assert_eq!(wire::error_code(&batches[offset + 3]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 3]),
            vec![vec![Some("42".to_owned())]],
            "{statement} connection recovery"
        );
    }
}

#[test]
fn should_decode_binary_bytea_parameters_without_text_reencoding() {
    // Arrange
    let sql = "SELECT encode($1::bytea, 'hex') AS payload_hex";
    let batches = run_script(
        "issue-816-binary-bytea-parameter",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("binary_bytea", sql, &[17])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("binary_bytea"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "binary_bytea_portal",
                    "binary_bytea",
                    &[1],
                    &[Some(&[0x00, 0xff])],
                    &[],
                ),
                wire::execute_frame("binary_bytea_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("00ff".to_owned())]]);
}

#[test]
fn should_decode_binary_bit_parameters_with_bit_length_header() {
    // Arrange
    let sql = "SELECT $1::bit(5) AS bits";
    let bit_payload = [0, 0, 0, 5, 0b1010_1000];
    let batches = run_script(
        "issue-816-binary-bit-parameter",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("binary_bit", sql, &[1560])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("binary_bit"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "binary_bit_portal",
                    "binary_bit",
                    &[1],
                    &[Some(&bit_payload)],
                    &[],
                ),
                wire::execute_frame("binary_bit_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("10101".to_owned())]]
    );
}

#[test]
fn should_encode_binary_bit_bytea_results() {
    // Arrange
    let sql = r"SELECT B'10101'::bit(5) AS bits, '\x00ff'::bytea AS payload";
    let batches = run_script(
        "issue-816-binary-result-codecs",
        vec![
            Step::ParseFlush(wire::parse_frame("binary_results", sql)),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "binary_result_portal",
                    "binary_results",
                    &[],
                    &[],
                    &[1],
                ),
                wire::describe_portal_frame("binary_result_portal"),
                wire::execute_frame("binary_result_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[1];
    let fields = row_description(result);

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        fields
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![1560, 17]
    );
    assert!(fields.iter().all(|field| field.format_code == 1));
    assert_eq!(
        data_row_bytes(result),
        vec![vec![vec![0, 0, 0, 5, 0b1010_1000], vec![0x00, 0xff]]]
    );
}

#[test]
fn should_order_bytea_values_by_unsigned_bytes() {
    // Arrange
    let sql = r"SELECT payload FROM (VALUES ('\x80'::bytea), ('\xff'::bytea), ('\x00ff'::bytea), ('\x7f'::bytea), ('\x00'::bytea)) AS inputs(payload) ORDER BY payload";
    let statement = "bytea_unsigned_order";
    let portal = "bytea_unsigned_order_binary";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-816-bytea-unsigned-order", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let columns = row_description(result);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(17, 1)]
    );
    assert_eq!(
        data_row_bytes(result),
        vec![
            vec![vec![0x00]],
            vec![vec![0x00, 0xff]],
            vec![vec![0x7f]],
            vec![vec![0x80]],
            vec![vec![0xff]]
        ]
    );
}

#[test]
fn should_preserve_nulls_for_binary_type_parameters() {
    // Arrange
    let sql = "SELECT $1::bit(3) AS bits, $2::varbit AS variable, $3::bytea AS payload";
    let batches = run_script(
        "issue-816-binary-null-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "binary_null_parameters",
                sql,
                &[1560, 1562, 17],
            )),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "binary_null_portal",
                    "binary_null_parameters",
                    &[0],
                    &[None, None, None],
                    &[],
                ),
                wire::execute_frame("binary_null_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![None, None, None]]);
}
