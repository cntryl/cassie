//! Scoped PostgreSQL TIMETZ `to_char` overload diagnostics.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_timetz_format_signatures_during_parse() {
    // Arrange
    let cases: [(&str, &[i32], &str); 4] = [
        ("SELECT to_char(NULL::timetz, 'HH24')", &[], "to_char"),
        ("SELECT to_char(NULL::timetz, NULL)", &[], "to_char"),
        (
            "SELECT pg_catalog.to_char(NULL::timetz, 'HH24')",
            &[],
            "pg_catalog.to_char",
        ),
        ("SELECT to_char($1, 'HH24')", &[1266], "to_char"),
    ];
    let mut steps = Vec::new();
    for (index, (sql, oids, _)) in cases.iter().enumerate() {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &format!("bad_{index}"),
            sql,
            oids,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT to_char(NULL::timetz, 'HH24')",
    )]));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 45",
    )]));

    // Act
    let batches = run_script("issue-815-timetz-format-parse", steps);

    // Assert
    for (index, (_, _, name)) in cases.iter().enumerate() {
        let error = &batches[index * 3];
        assert_eq!(error.first().map(|frame| frame.0), Some(b'E'));
        assert_eq!(wire::error_code(error).as_deref(), Some("42883"));
        let fields = wire::parse_error_fields(&error[0].1);
        assert!(fields.iter().any(|(tag, value)| *tag == 'M'
            && value == &format!("function {name}(time with time zone, unknown) does not exist")));
        assert!(fields.iter().any(|(tag, value)| *tag == 'H' && value == "No function matches the given name and argument types. You might need to add explicit type casts."));
        assert_eq!(
            batches[index * 3 + 1].last().map(|frame| frame.0),
            Some(b'Z')
        );
        assert_eq!(wire::error_code(&batches[index * 3 + 2]), None);
    }
    let simple = &batches[cases.len() * 3];
    assert_eq!(wire::error_code(simple).as_deref(), Some("42883"));
    assert!(wire::parse_error_fields(&simple.iter().find(|frame| frame.0 == b'E').expect("simple error").1)
        .iter().any(|(tag, value)| *tag == 'H' && value == "No function matches the given name and argument types. You might need to add explicit type casts."));
    assert_eq!(wire::error_code(&batches[cases.len() * 3 + 1]), None);
}

#[test]
fn should_preserve_timetz_casts_outside_scoped_rejection() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "invalid_cast",
            "SELECT to_char('invalid'::timetz, 'HH24')",
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION to_char(v TIMETZ, fmt TEXT) RETURNS TEXT AS "'owned'""#,
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "override",
            "SELECT to_char(NULL::timetz, 'HH24')",
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];

    // Act
    let batches = run_script("issue-815-timetz-format-override", steps);

    // Assert
    assert_ne!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(batches[3].first().map(|frame| frame.0), Some(b'1'));
}

#[test]
fn should_fail_explicit_transaction_after_timetz_parse_error() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("BEGIN")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "bad_tx",
            "SELECT to_char(NULL::timetz, 'HH24')",
            &[],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
        Step::ThroughReady(vec![wire::simple_query_frame("ROLLBACK")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
    ];

    // Act
    let batches = run_script("issue-815-timetz-format-transaction", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(&batches[3]).as_deref(), Some("25P02"));
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("45".to_owned())]]
    );
}
