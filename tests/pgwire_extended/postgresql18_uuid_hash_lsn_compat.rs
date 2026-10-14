//! PostgreSQL 18 UUID, hash, and `pg_lsn` protocol contracts tracked by #816.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn binary_data_rows(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<Option<Vec<u8>>>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| {
            let field_count = i16::from_be_bytes(
                payload[..2]
                    .try_into()
                    .expect("DataRow field count should be present"),
            );
            let mut cursor = 2_usize;
            (0..field_count)
                .map(|_| {
                    let len = i32::from_be_bytes(
                        payload[cursor..cursor + 4]
                            .try_into()
                            .expect("DataRow field length should be present"),
                    );
                    cursor += 4;
                    if len < 0 {
                        None
                    } else {
                        let end = cursor + usize::try_from(len).expect("field length is positive");
                        let value = payload[cursor..end].to_vec();
                        cursor = end;
                        Some(value)
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn should_preserve_uuid_parameter_codec_contract() {
    // Arrange
    let uuid_bytes = [
        0xa0, 0xee, 0xbc, 0x99, 0x9c, 0x0b, 0x4e, 0xf8, 0xbb, 0x6d, 0x6b, 0xb9, 0xbd, 0x38, 0x0a,
        0x11,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "uuid_value",
            "SELECT $1::UUID",
            &[2950],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("uuid_value"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "uuid_text_portal",
                "uuid_value",
                &["A0EEBC999C0B4EF8BB6D6BB9BD380A11"],
            ),
            wire::describe_portal_frame("uuid_text_portal"),
            wire::execute_frame("uuid_text_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "uuid_binary_portal",
                "uuid_value",
                &[1],
                &[Some(&uuid_bytes)],
                &[1],
            ),
            wire::describe_portal_frame("uuid_binary_portal"),
            wire::execute_frame("uuid_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-uuid-codecs", steps);

    // Assert
    assert_eq!(batches[0][0].0, b'1');
    let statement_description = &batches[1];
    let parameters = statement_description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("UUID ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![2950]);
    let result_description = statement_description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UUID RowDescription");
    let uuid_column = &wire::parse_row_description(&result_description.1)[0];
    assert_eq!(uuid_column.type_oid, 2950);
    assert_eq!(uuid_column.type_size, 16);

    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some(
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_owned()
        )]]
    );
    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary UUID RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(binary_column.type_oid, 2950);
    assert_eq!(binary_column.format_code, 1);
    assert_eq!(
        binary_data_rows(&batches[3]),
        vec![vec![Some(uuid_bytes.to_vec())]]
    );
}

#[test]
fn should_preserve_pg_lsn_parameter_codec_contract() {
    // Arrange
    let lsn_bytes = [0x00, 0x00, 0x00, 0x16, 0xb3, 0x74, 0xd8, 0x48];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "lsn_value",
            "SELECT $1::PG_LSN",
            &[3220],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("lsn_value"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("lsn_text_portal", "lsn_value", &["16/B374D848"]),
            wire::describe_portal_frame("lsn_text_portal"),
            wire::execute_frame("lsn_text_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "lsn_binary_portal",
                "lsn_value",
                &[1],
                &[Some(&lsn_bytes)],
                &[1],
            ),
            wire::describe_portal_frame("lsn_binary_portal"),
            wire::execute_frame("lsn_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-pg-lsn-codecs", steps);

    // Assert
    assert_eq!(batches[0][0].0, b'1');
    let statement_description = &batches[1];
    let parameters = statement_description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("pg_lsn ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![3220]);
    let result_description = statement_description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("pg_lsn RowDescription");
    let lsn_column = &wire::parse_row_description(&result_description.1)[0];
    assert_eq!(lsn_column.type_oid, 3220);
    assert_eq!(lsn_column.type_size, 8);

    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("16/B374D848".to_owned())]]
    );
    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary pg_lsn RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(binary_column.type_oid, 3220);
    assert_eq!(binary_column.format_code, 1);
    assert_eq!(
        binary_data_rows(&batches[3]),
        vec![vec![Some(lsn_bytes.to_vec())]]
    );
}

#[test]
fn should_describe_uuid_function_results_with_postgresql_oids() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT gen_random_uuid(), uuidv4(), uuidv7(), uuid_extract_version(gen_random_uuid()), uuid_extract_version(uuidv4()), uuid_extract_version(uuidv7()), uuid_extract_timestamp(uuidv7())",
    )])];

    // Act
    let batches = run_script("postgresql18-uuid-function-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UUID function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2950, 2950, 2950, 21, 21, 21, 1184]
    );
    let rows = wire::data_rows(&batches[0]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0][..3].iter().all(Option::is_some));
    assert_eq!(
        &rows[0][3..6],
        &[
            Some("4".to_owned()),
            Some("4".to_owned()),
            Some("7".to_owned())
        ]
    );
    assert!(rows[0][6].is_some());
}

#[test]
fn should_describe_hash_function_results_with_postgresql_oids() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE EXTENSION pgcrypto")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT md5('abc'), sha256('abc'::BYTEA), crc32('abc'::BYTEA), digest('abc', 'sha256'), hashtext('abc'), hashtextextended('abc', 0)",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-hash-function-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("hash function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 17, 20, 17, 23, 20]
    );
    assert_eq!(wire::data_rows(&batches[1]).len(), 1);
}

#[test]
fn should_describe_crc32c_result_with_postgresql_bigint_identity() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT crc32c(convert_to('abc', 'UTF8'))",
    )])];

    // Act
    let batches = run_script("postgresql18-crc32c-result-identity", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "PostgreSQL 18 CRC32C must be callable without an extension"
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("CRC32C RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(column.type_oid, 20);
    assert_eq!(column.type_size, 8);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("910901175".to_owned())]]
    );
}

#[test]
fn should_bind_hash_function_parameters_with_postgresql_types() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "text_hashes",
            "SELECT hashtext($1), hashtextextended($1, $2)",
            &[25, 20],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("text_hashes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "text_hash_portal",
                "text_hashes",
                &["test string of type text", "0"],
            ),
            wire::describe_portal_frame("text_hash_portal"),
            wire::execute_frame("text_hash_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-hash-parameter-oids", steps);

    // Assert
    assert_eq!(batches[0][0].0, b'1');
    let description = &batches[1];
    let parameters = description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("hash ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 20]
    );
    let result_description = description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("hash RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 20]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::data_rows(&batches[2]).len(), 1);
}

#[test]
fn should_describe_nullable_uuid_lsn_values_with_their_type_oids() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::UUID, NULL::PG_LSN",
    )])];

    // Act
    let batches = run_script("postgresql18-uuid-pg-lsn-null-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("nullable type RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2950, 3220]
    );
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None, None]]);
}

#[test]
fn should_reject_invalid_digest_algorithm_then_recover() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE EXTENSION pgcrypto")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT digest('abc', 'not-a-supported-hash')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
    ];

    // Act
    let batches = run_script("postgresql18-digest-error-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_gate_digest_on_pgcrypto_extension_creation() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT digest('abc', 'sha256')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE EXTENSION pgcrypto")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT digest('abc', 'sha256')",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-pgcrypto-digest-gate", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("digest RowDescription after extension creation");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some(
            "\\xba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned()
        )]]
    );
}
