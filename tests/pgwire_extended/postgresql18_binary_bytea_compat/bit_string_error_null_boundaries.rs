use super::*;

#[test]
fn should_describe_null_bit_string_function_results_over_pgwire() {
    // Arrange
    let sql = "SELECT NULL::bit varying || B'1', \
                      NULL::bit(1) & B'1', \
                      NULL::bit(1) | B'1', \
                      NULL::bit(1) # B'1', \
                      ~NULL::bit, \
                      NULL::bit(3) << 1, \
                      NULL::bit(3) >> 1, \
                      bit_count(NULL::bit varying), \
                      bit_length(NULL::bit), \
                      length(NULL::bit), \
                      octet_length(NULL::bit), \
                      position(B'1' IN NULL::bit varying), \
                      substring(NULL::bit FROM 1), \
                      overlay(NULL::bit PLACING B'1' FROM 1), \
                      get_bit(NULL::bit varying, 0), \
                      set_bit(NULL::bit varying, 0, 1)";

    // Act
    let batches = run_script(
        "issue-816-bit-string-null-function-results",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![
            1562, 1560, 1560, 1560, 1560, 1560, 1560, 20, 23, 23, 23, 23, 1560, 1560, 23, 1560,
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
            None, None,
        ]]
    );
}

#[test]
fn should_preserve_shift_substring_overlay_boundary_results() {
    // Arrange
    let sql = "SELECT B'10110' << 0, \
                      B'10110' << 5, \
                      B'10110' << 8, \
                      B'10110' >> -2, \
                      B'10110' >> 5, \
                      B'10110' >> 8, \
                      substring(B'110101' FROM 0 FOR 4), \
                      substring(B'110101' FROM -2 FOR 4), \
                      substring(B'110101' FROM 8 FOR 2), \
                      substring(B'110101' FROM 3), \
                      substring(B'110101' FOR 3), \
                      overlay(B'101101' PLACING B'00' FROM 3), \
                      overlay(B'101101' PLACING B'111' FROM 2 FOR 1), \
                      overlay(B'101101' PLACING B'1' FROM 8)";

    // Act
    let batches = run_script(
        "issue-816-bit-string-shift-slice-boundaries",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![1560; 14]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("10110".to_owned()),
            Some("00000".to_owned()),
            Some("00000".to_owned()),
            Some("11000".to_owned()),
            Some("00000".to_owned()),
            Some("00000".to_owned()),
            Some("110".to_owned()),
            Some("1".to_owned()),
            Some(String::new()),
            Some("0101".to_owned()),
            Some("110".to_owned()),
            Some("100001".to_owned()),
            Some("11111101".to_owned()),
            Some("1011011".to_owned()),
        ]]
    );
}

#[test]
fn should_return_sqlstate_errors_with_recovery_for_bit_string_calls() {
    // Arrange
    let error_cases = [
        (
            "and requires equal bit widths",
            "SELECT B'101' & B'10'",
            "22026",
        ),
        (
            "or requires equal bit widths",
            "SELECT B'101' | B'10'",
            "22026",
        ),
        (
            "xor requires equal bit widths",
            "SELECT B'101' # B'10'",
            "22026",
        ),
        (
            "get_bit rejects a negative index",
            "SELECT get_bit(B'101', -1)",
            "2202E",
        ),
        (
            "get_bit rejects the first index after the value",
            "SELECT get_bit(B'101', 3)",
            "2202E",
        ),
        (
            "set_bit rejects a negative index",
            "SELECT set_bit(B'101', -1, 0)",
            "2202E",
        ),
        (
            "set_bit rejects the first index after the value",
            "SELECT set_bit(B'101', 3, 1)",
            "2202E",
        ),
        (
            "set_bit accepts only zero or one",
            "SELECT set_bit(B'101', 1, 2)",
            "22023",
        ),
        (
            "overlay rejects a zero start position",
            "SELECT overlay(B'101' PLACING B'1' FROM 0)",
            "22011",
        ),
        (
            "substring rejects a negative length",
            "SELECT substring(B'101' FROM 1 FOR -1)",
            "22011",
        ),
    ];
    let mut steps = Vec::with_capacity(error_cases.len() * 2);
    for (_, sql, _) in error_cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42 AS recovered",
        )]));
    }

    // Act
    let batches = run_script("issue-816-bit-string-error-recovery", steps);

    // Assert
    assert_eq!(batches.len(), error_cases.len() * 2);
    for (index, (label, sql, sqlstate)) in error_cases.iter().enumerate() {
        let error_frames = &batches[index * 2];
        assert_eq!(
            wire::error_code(error_frames).as_deref(),
            Some(*sqlstate),
            "PostgreSQL 18.6 SQLSTATE for {label}: {sql}"
        );
        assert!(
            error_frames.iter().any(|(tag, _)| *tag == b'E'),
            "{label} should return ErrorResponse"
        );
        assert!(
            !error_frames
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
            "{label} must not return partial result frames"
        );
        assert_eq!(
            error_frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "{label} should restore ReadyForQuery idle state"
        );

        let recovered = &batches[index * 2 + 1];
        assert_eq!(wire::error_code(recovered), None, "{label} recovery");
        assert_eq!(
            wire::data_rows(recovered),
            vec![vec![Some("42".to_owned())]],
            "{label} should permit a following query"
        );
    }
}
