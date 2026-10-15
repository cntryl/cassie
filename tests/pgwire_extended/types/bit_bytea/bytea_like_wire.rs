//! PostgreSQL 18 bytea LIKE operator and escape-function identities for #816.

use super::{row_description, run_script, wire, Step};

#[test]
fn should_match_bytea_like_patterns_as_binary_bytes() {
    // Arrange
    let sql = concat!(
        r"SELECT '\x00ff'::bytea LIKE '\x5f5f'::bytea AS two_bytes, ",
        r"'\x00ff'::bytea LIKE '\x0025'::bytea AS zero_prefix"
    );
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-816-bytea-like-byte-semantics", steps);
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "bytea LIKE ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1); 2]
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned()); 2]]);
}

#[test]
fn should_apply_bytea_like_escape_semantics_over_pgwire() {
    // Arrange
    let sql = r"SELECT '\x615f62'::bytea LIKE '\x615c5f62'::bytea ESCAPE '\x5c'::bytea AS escaped_underscore";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-816-bytea-like-escape-semantics", steps);
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "bytea LIKE ESCAPE ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1)]
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_describe_bytea_like_escape_result_with_bytea_oid() {
    // Arrange
    let statement = "bytea_like_escape_result";
    let portal = "bytea_like_escape_result_portal";
    let sql = "SELECT like_escape($1::bytea, $2::bytea) AS escaped_pattern";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[17, 17])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &[r"\x615f62", r"\x5f"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-816-bytea-like-escape-result-identity", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "bytea like_escape Parse ErrorResponse fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(&batches[1]),
        None,
        "bytea like_escape Describe ErrorResponse fields: {:?}",
        batches[1]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("bytea like_escape ParameterDescription")
                .1
        ),
        vec![17, 17]
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "bytea like_escape Bind ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(17, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x615c62".to_owned())]]
    );
}
