//! PostgreSQL 18 UUID, hash, and `pg_lsn` protocol contracts tracked by #816.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

mod alternative_input_forms;
mod text_input_error_sqlstates;

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

fn hex_bytes(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(
                std::str::from_utf8(pair).expect("hex bytes should be UTF-8"),
                16,
            )
            .expect("value should contain hexadecimal byte pairs")
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
fn should_preserve_uuid_binary_boundary_encodings() {
    // Arrange
    let sql = "SELECT '00000000-0000-0000-0000-000000000000'::UUID AS uuid_min, 'ffffffff-ffff-ffff-ffff-ffffffffffff'::UUID AS uuid_max, '00000000-0000-0000-0000-000000000000'::UUID < 'ffffffff-ffff-ffff-ffff-ffffffffffff'::UUID AS uuid_order";
    let statement = "uuid_boundary_values";
    let portal = "uuid_boundary_values_binary";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-uuid-wire-boundaries", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let text_description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UUID text RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(2950, 16), (2950, 16), (16, 1)]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("00000000-0000-0000-0000-000000000000".to_owned()),
            Some("ffffffff-ffff-ffff-ffff-ffffffffffff".to_owned()),
            Some("t".to_owned()),
        ]]
    );

    let binary_result = &batches[2];
    assert_eq!(wire::error_code(binary_result), None);
    let binary_description = binary_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UUID binary RowDescription");
    assert_eq!(
        wire::parse_row_description(&binary_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(2950, 16, 1), (2950, 16, 1), (16, 1, 1)]
    );
    assert_eq!(
        binary_data_rows(binary_result),
        vec![vec![
            Some(vec![0_u8; 16]),
            Some(vec![u8::MAX; 16]),
            Some(vec![1]),
        ]]
    );
}

#[test]
fn should_preserve_pg_lsn_binary_boundary_encodings() {
    // Arrange
    let sql = "SELECT '0/0'::PG_LSN AS lsn_min, 'FFFFFFFF/FFFFFFFF'::PG_LSN AS lsn_max, '0/0'::PG_LSN < 'FFFFFFFF/FFFFFFFF'::PG_LSN AS lsn_order";
    let statement = "pg_lsn_boundary_values";
    let portal = "pg_lsn_boundary_values_binary";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-pg-lsn-wire-boundaries", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let text_description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("pg_lsn text RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(3220, 8), (3220, 8), (16, 1)]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("0/0".to_owned()),
            Some("FFFFFFFF/FFFFFFFF".to_owned()),
            Some("t".to_owned()),
        ]]
    );

    let binary_result = &batches[2];
    assert_eq!(wire::error_code(binary_result), None);
    let binary_description = binary_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("pg_lsn binary RowDescription");
    assert_eq!(
        wire::parse_row_description(&binary_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(3220, 8, 1), (3220, 8, 1), (16, 1, 1)]
    );
    assert_eq!(
        binary_data_rows(binary_result),
        vec![vec![
            Some(vec![0_u8; 8]),
            Some(vec![u8::MAX; 8]),
            Some(vec![1]),
        ]]
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
fn should_return_hash_function_text_binary_vectors() {
    // Arrange
    let sql = "SELECT md5('abc'), sha256('abc'::BYTEA), crc32('abc'::BYTEA), \
               crc32c(convert_to('abc', 'UTF8')), digest('abc', 'sha256'), \
               hashtext('abc'), hashtextextended('abc', 0)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE EXTENSION pgcrypto")]),
        Step::ParseFlush(wire::parse_frame("hash_result_codecs", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "hash_result_codecs_portal",
                "hash_result_codecs",
                &[],
                &[],
                &[0, 1, 1, 1, 1, 1, 1],
            ),
            wire::describe_portal_frame("hash_result_codecs_portal"),
            wire::execute_frame("hash_result_codecs_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-hash-text-binary-vectors", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("hash result RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![
            (25, -1, 0),
            (17, -1, 1),
            (20, 8, 1),
            (20, 8, 1),
            (17, -1, 1),
            (23, 4, 1),
            (20, 8, 1),
        ]
    );
    assert_eq!(
        binary_data_rows(&batches[2]),
        vec![vec![
            Some(b"900150983cd24fb0d6963f7d28e17f72".to_vec()),
            Some(vec![
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]),
            Some(891_568_578_i64.to_be_bytes().to_vec()),
            Some(910_901_175_i64.to_be_bytes().to_vec()),
            Some(vec![
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]),
            Some((-785_388_649_i32).to_be_bytes().to_vec()),
            Some((-6_747_756_470_228_489_321_i64).to_be_bytes().to_vec()),
        ]]
    );
}

#[test]
fn should_return_sha2_family_text_binary_vectors() {
    // Arrange
    let sql = "SELECT sha224('abc'::BYTEA), sha224('abc'::BYTEA), \
               sha384('abc'::BYTEA), sha384('abc'::BYTEA), \
               sha512('abc'::BYTEA), sha512('abc'::BYTEA)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("sha2_result_codecs", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "sha2_result_codecs_portal",
                "sha2_result_codecs",
                &[],
                &[],
                &[0, 1, 0, 1, 0, 1],
            ),
            wire::describe_portal_frame("sha2_result_codecs_portal"),
            wire::execute_frame("sha2_result_codecs_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-sha2-text-binary-vectors", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("SHA-2 result RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![
            (17, -1, 0),
            (17, -1, 1),
            (17, -1, 0),
            (17, -1, 1),
            (17, -1, 0),
            (17, -1, 1),
        ]
    );
    assert_eq!(
        binary_data_rows(&batches[1]),
        vec![vec![
            Some(br"\x23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7".to_vec()),
            Some(hex_bytes(
                "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7",
            )),
            Some(br"\xcb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7".to_vec()),
            Some(hex_bytes(
                "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
            )),
            Some(br"\xddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f".to_vec()),
            Some(hex_bytes(
                "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
            )),
        ]]
    );
}

#[test]
fn should_report_pg_lsn_overflow_underflow_sqlstates() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'FFFFFFFF/FFFFFFFF'::PG_LSN + 1::NUMERIC",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT '0/0'::PG_LSN - 1::NUMERIC",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
    ];

    // Act
    let batches = run_script("postgresql18-pg-lsn-range-sqlstates", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22023"));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_report_numeric_metadata_for_pg_wal_lsn_diff_across_word_boundaries() {
    // Arrange
    let sql = "SELECT pg_wal_lsn_diff('1/0'::PG_LSN, '0/FFFFFFFF'::PG_LSN) AS forward, \
               pg_wal_lsn_diff('0/FFFFFFFF'::PG_LSN, '1/0'::PG_LSN) AS backward";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-pg-wal-lsn-diff-word-boundary", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("pg_wal_lsn_diff RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1), (1700, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned()), Some("-1".to_owned())]]
    );
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
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("-1790427109".to_owned()),
            Some("-6578719834206879717".to_owned()),
        ]]
    );
}

#[test]
fn should_bind_binary_hash_parameters_with_postgresql_vectors() {
    // Arrange
    let statement = "binary_hash_parameters";
    let seed = 0_i64.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT hashtext($1), hashtextextended($1, $2)",
            &[25, 20],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "binary_hash_portal",
                statement,
                &[1],
                &[Some(b"abc"), Some(&seed)],
                &[1],
            ),
            wire::describe_portal_frame("binary_hash_portal"),
            wire::execute_frame("binary_hash_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-hash-binary-parameters", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let description = &batches[1];
    let parameters = description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("binary hash ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 20]
    );
    let result_description = description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary hash RowDescription");
    let columns = wire::parse_row_description(&result_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.format_code))
            .collect::<Vec<_>>(),
        vec![(23, 1), (20, 1)]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        binary_data_rows(&batches[2]),
        vec![vec![
            Some((-785_388_649_i32).to_be_bytes().to_vec()),
            Some((-6_747_756_470_228_489_321_i64).to_be_bytes().to_vec()),
        ]]
    );
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
