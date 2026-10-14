use super::support_parameter_occurrence_wire::{Step, run_script};
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_report_mixed_conditional_type_sqlstate_before_scanning() {
    // Arrange
    let batches = run_script(
        "issue-802-incompatible-types-before-scan",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_802_empty_types (number INT, flag BOOLEAN)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT GREATEST(number, flag) FROM issue_802_empty_types",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_802_empty_types",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let invalid = &batches[1];
    let empty_scan = &batches[2];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42804"));
    let error = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("incompatible GREATEST types ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(
        fields.iter().any(|(field, message)| {
            *field == 'M' && message == "GREATEST types integer and boolean cannot be matched"
        }),
        "must reach common-type resolution: {fields:?}"
    );
    assert!(wire::data_rows(invalid).is_empty());
    assert_eq!(wire::error_code(empty_scan), None);
    assert_eq!(
        wire::data_rows(empty_scan),
        vec![vec![Some("0".to_owned())]]
    );
}

#[test]
fn should_preserve_conditional_descriptors_across_wire_formats() {
    // Arrange
    let cases = [
        (
            "SELECT NULLIF(CAST(1 AS INT), CAST(2 AS BIGINT)) AS v",
            (23, 4, -1),
            "1",
            Some(1_i32.to_be_bytes().to_vec()),
        ),
        (
            "SELECT NULLIF(CAST(1 AS INT), CAST(2.2 AS FLOAT)) AS v",
            (701, 8, -1),
            "1",
            Some(1_f64.to_be_bytes().to_vec()),
        ),
        (
            "SELECT GREATEST(CAST(1 AS INT), CAST(NULL AS FLOAT)) AS v",
            (701, 8, -1),
            "1",
            Some(1_f64.to_be_bytes().to_vec()),
        ),
        ("SELECT GREATEST(NULL, NULL) AS v", (25, -1, -1), "", None),
        (
            "SELECT NULLIF(COALESCE(CAST(9007199254740993 AS BIGINT), CAST(NULL AS FLOAT)), CAST(NULL AS FLOAT)) AS v",
            (701, 8, -1),
            "9007199254740992",
            Some(9_007_199_254_740_992_f64.to_be_bytes().to_vec()),
        ),
        (
            "WITH derived AS (SELECT COALESCE(CAST(9007199254740993 AS BIGINT), CAST(NULL AS FLOAT)) AS v) SELECT GREATEST(v, CAST(NULL AS FLOAT)) AS v FROM derived",
            (701, 8, -1),
            "9007199254740992",
            Some(9_007_199_254_740_992_f64.to_be_bytes().to_vec()),
        ),
    ];
    let cycles = [0, 1]
        .into_iter()
        .flat_map(|format| {
            cases
                .iter()
                .map(move |(sql, _, _, _)| fixture::execute_cycle(sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("conditional-wire-descriptors", &[], cycles);

    // Assert
    for (format, group) in [0, 1].into_iter().zip(batches.chunks(cases.len())) {
        for ((_, descriptor, text, binary), frames) in cases.iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, *descriptor, format, true);
            if format == 0 {
                assert_eq!(
                    wire::data_rows(frames),
                    vec![vec![binary.as_ref().map(|_| text.to_string())]]
                );
            } else {
                let mut expected = vec![0, 1];
                if let Some(binary) = binary {
                    expected.extend_from_slice(
                        &i32::try_from(binary.len())
                            .expect("scalar length")
                            .to_be_bytes(),
                    );
                    expected.extend_from_slice(binary);
                } else {
                    expected.extend_from_slice(&(-1_i32).to_be_bytes());
                }
                assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
            }
        }
    }
}

#[test]
fn should_preserve_cast_bound_conditional_parameter_metadata() {
    // Arrange
    let sql = "SELECT GREATEST(CAST($1 AS INT), CAST(7 AS BIGINT)) AS v";
    let parameter = 2_i32.to_be_bytes();
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, Some((23, 1, Some(&parameter))), format, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("conditional-wire-parameters", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (20, 8, -1), format, true);
        if format == 0 {
            assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_string())]]);
        } else {
            let mut expected = vec![0, 1, 0, 0, 0, 8];
            expected.extend_from_slice(&7_i64.to_be_bytes());
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_preserve_conditional_normalization_during_parameter_revalidation() {
    // Arrange
    let sql = "SELECT GREATEST(CAST($1 AS FLOAT) * 10, 0) AS v";
    let parameter = 1e308_f64.to_be_bytes();
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, Some((701, 1, Some(&parameter))), format, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("conditional-wire-revalidation", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
        if format == 0 {
            assert_eq!(
                wire::data_rows(frames),
                vec![vec![Some("Infinity".to_string())]]
            );
        } else {
            let mut expected = vec![0, 1, 0, 0, 0, 8];
            expected.extend_from_slice(&f64::INFINITY.to_be_bytes());
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}
