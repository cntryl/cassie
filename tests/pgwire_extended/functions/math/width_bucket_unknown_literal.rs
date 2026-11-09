//! Unknown operand parameters with a literal WIDTH_BUCKET count.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_infer_unknown_operands_with_a_literal_or_null_count() {
    // Arrange
    let cases: [(&str, &[i32], &[&str], Option<&str>); 3] = [
        (
            "SELECT WIDTH_BUCKET($1, $2, $3, '2')",
            &[701, 701, 701],
            &["0.5", "0", "1"],
            Some("2"),
        ),
        (
            "SELECT WIDTH_BUCKET($1, $1, $2, '0x2')",
            &[701, 701],
            &["0", "1"],
            Some("1"),
        ),
        (
            "SELECT WIDTH_BUCKET($1, $2, $3, NULL)",
            &[701, 701, 701],
            &["0.5", "0", "1"],
            None,
        ),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _, values, _)) in cases.iter().enumerate() {
        let statement = format!("bucket_unknown_literal_{index}");
        let portal = format!("bucket_unknown_literal_portal_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame(&statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(&portal, &statement, values),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-unknown-literal", steps);

    // Assert
    for (index, (_, types, _, value)) in cases.iter().enumerate() {
        let offset = index * 3;
        assert_eq!(batches[offset].first().map(|frame| frame.0), Some(b'1'));
        let parameters = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1).as_slice(),
            *types
        );
        assert_eq!(wire::error_code(&batches[offset + 2]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 2]),
            vec![vec![value.map(str::to_owned)]]
        );
    }
}

#[test]
fn should_report_bad_literal_count_before_unknown_parameter_binding() {
    // Arrange
    let cases = [("bad", "22P02"), ("2147483648", "22003")];
    let mut steps = Vec::new();
    for (index, (literal, _)) in cases.iter().enumerate() {
        let sql = format!("SELECT WIDTH_BUCKET($1, $2, $3, '{literal}')");
        steps.push(Step::ParseFlush(wire::parse_frame(
            &format!("bucket_bad_literal_{index}"),
            &sql,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-unknown-bad-literal", steps);

    // Assert
    for (index, (_, state)) in cases.iter().enumerate() {
        assert_eq!(batches[index * 3].first().map(|frame| frame.0), Some(b'E'));
        assert_eq!(
            wire::error_code(&batches[index * 3]).as_deref(),
            Some(*state)
        );
        assert_eq!(
            wire::data_rows(&batches[index * 3 + 2]),
            vec![vec![Some("45".to_owned())]]
        );
    }
}

#[test]
fn should_preserve_later_cast_and_visible_routine_selection() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "bucket_literal_later",
            "SELECT WIDTH_BUCKET($1, $2, $3, '2'), $1::text",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_literal_later"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION width_bucket(a TEXT, b TEXT, c TEXT, d TEXT) RETURNS TEXT AS "d""#,
        )]),
        Step::ParseFlush(wire::parse_frame(
            "bucket_literal_routine",
            "SELECT WIDTH_BUCKET($1, $2, $3, '2')",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_literal_routine"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-813-width-bucket-unknown-literal-context", steps);

    // Assert
    for (index, expected) in [(1, [701, 701, 701]), (4, [25, 25, 25])] {
        let parameters = batches[index]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), expected);
    }
    assert_eq!(wire::error_code(&batches[2]), None);
}
