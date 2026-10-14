//! Selected OID705 descriptor authority; NULL value framing is independent.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use crate::support_parameter_occurrence_wire::{Step, run_script};

#[test]
fn should_match_unknown_carrier_lengths_across_metadata_surfaces() {
    // Arrange
    let setup = [("CREATE TABLE metadata_empty (k INT)", Vec::new())];
    let cases = [
        ("SELECT NULL AS v", true),
        ("SELECT NULL AS v FROM metadata_empty", false),
    ];
    let mut cycles = vec![fixture::execute_cycle(
        "SELECT typlen FROM pg_catalog.pg_type WHERE oid = 705",
        None,
        0,
        false,
    )];
    for format in [0, 1] {
        cycles.extend(cases.map(|(sql, _)| fixture::execute_cycle(sql, None, format, true)));
    }

    // Act
    let batches = fixture::run_cycles("type-unknown-length", &setup, cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("-2".to_string())]]
    );
    for (format, pair) in [0, 1].into_iter().zip(batches[1..].as_chunks::<2>().0) {
        for ((_, has_row), frames) in cases.into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (705, -2, -1), format, true);
            let expected = if has_row {
                vec![vec![0, 1, 255, 255, 255, 255]]
            } else {
                Vec::new()
            };
            assert_eq!(fixture::data_row_payloads(frames), expected);
        }
    }
}

#[test]
fn should_preserve_declared_null_result_descriptors() {
    // Arrange
    let setup = [("CREATE TABLE metadata_empty (k INT)", Vec::new())];
    let cases = [
        ("SELECT CAST(NULL AS INT) AS v", (23, 4, -1), true),
        ("SELECT CAST(NULL AS BOOLEAN) AS v", (16, 1, -1), true),
        ("SELECT CAST(NULL AS JSON) AS v", (114, -1, -1), true),
        (
            "SELECT CAST(NULL AS INT) AS v FROM metadata_empty",
            (23, 4, -1),
            false,
        ),
        (
            "SELECT CAST(NULL AS BOOLEAN) AS v FROM metadata_empty",
            (16, 1, -1),
            false,
        ),
        (
            "SELECT CAST(NULL AS JSON) AS v FROM metadata_empty",
            (114, -1, -1),
            false,
        ),
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| cases.map(|(sql, _, _)| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-declared-null", &setup, cycles);

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.as_chunks::<6>().0) {
        for ((_, descriptor, has_row), frames) in cases.into_iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, descriptor, format, true);
            let expected = if has_row {
                vec![vec![0, 1, 255, 255, 255, 255]]
            } else {
                Vec::new()
            };
            assert_eq!(fixture::data_row_payloads(frames), expected);
        }
    }
}

#[test]
fn should_infer_oid_zero_integer_parameter_for_binary_bind() {
    // Arrange
    let int4 = 1_i32.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "oid_zero_int4_comparison",
            "SELECT $1 = 1",
            &[0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("oid_zero_int4_comparison"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "oid_zero_int4_portal",
                "oid_zero_int4_comparison",
                &[1],
                &[Some(&int4)],
                &[],
            ),
            wire::describe_portal_frame("oid_zero_int4_portal"),
            wire::execute_frame("oid_zero_int4_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("type-oid-zero-int4-binary-bind", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let statement_description = &batches[1];
    let parameter_types = statement_description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![23]
    );
    let result_description = statement_description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        16
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("t".to_owned())]]
    );
}

#[test]
fn should_infer_coalesce_result_types_for_all_null_inputs() {
    // Arrange
    let sql = "SELECT COALESCE(NULL, NULL) AS all_null_value, COALESCE(NULL, 1) AS unknown_value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-all-null-unknown-inference", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let expected_columns = vec![
        wire::RowDescription {
            name: "all_null_value".into(),
            table_oid: 0,
            attr_num: 0,
            type_oid: 25,
            type_size: -1,
            type_mod: -1,
            format_code: 0,
        },
        wire::RowDescription {
            name: "unknown_value".into(),
            table_oid: 0,
            attr_num: 0,
            type_oid: 23,
            type_size: 4,
            type_mod: -1,
            format_code: 0,
        },
    ];
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(
        descriptions,
        vec![expected_columns.clone(), expected_columns]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![None, Some("1".to_owned())]]
    );
}

#[test]
fn should_infer_text_type_for_an_all_null_case_expression() {
    // Arrange
    let sql = "SELECT CASE WHEN TRUE THEN NULL ELSE NULL END AS value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-unknown-case-inference", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2, "statement and portal describe");
    assert_eq!(descriptions[0][0].type_oid, 25, "PostgreSQL text OID");
    assert_eq!(descriptions[1][0].type_oid, 25, "PostgreSQL text OID");
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}

#[test]
fn should_infer_integer_type_for_values_with_an_unknown_null_row() {
    // Arrange
    let sql = "SELECT value FROM (VALUES (NULL), (1)) AS input(value) LIMIT 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-unknown-values-inference", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2, "statement and portal describe");
    assert_eq!(descriptions[0][0].type_oid, 23, "PostgreSQL int4 OID");
    assert_eq!(descriptions[1][0].type_oid, 23, "PostgreSQL int4 OID");
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}

#[test]
fn should_infer_integer_type_for_a_union_with_an_unknown_null_row() {
    // Arrange
    let sql = "SELECT value FROM (SELECT NULL AS value UNION ALL SELECT 1) AS input LIMIT 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-unknown-union-inference", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2, "statement and portal describe");
    assert_eq!(descriptions[0][0].type_oid, 23, "PostgreSQL int4 OID");
    assert_eq!(descriptions[1][0].type_oid, 23, "PostgreSQL int4 OID");
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}
