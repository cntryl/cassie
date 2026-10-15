//! PostgreSQL aggregate DISTINCT ordering validation over pgwire for issue #804.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_distinct_aggregate_order_key_outside_arguments_with_42p10() {
    // Arrange
    let batches = run_script(
        "issue-804-distinct-aggregate-order-key-validation",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT string_agg(DISTINCT v, ',' ORDER BY v) \
                 FROM (VALUES ('b', 2), ('a', 1), ('b', 3)) AS input(v, k)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT string_agg(DISTINCT v, ',' ORDER BY k) \
                 FROM (VALUES ('b', 2), ('a', 1)) AS input(v, k)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    let description = valid
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription for valid DISTINCT ordered string_agg");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("a,b".to_owned())]]);

    assert_eq!(wire::error_code(invalid).as_deref(), Some("42P10"));
    let error = invalid
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("aggregate DISTINCT ORDER BY validation ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, message)| message.as_str()),
        Some("in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list")
    );
    assert_eq!(wire::data_rows(invalid), Vec::<Vec<Option<String>>>::new());
    assert_eq!(invalid.iter().filter(|frame| frame.0 == b'C').count(), 0);

    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_preserve_modifier_semantics_given_real_tables_and_scoped_scalars_when_simple_and_parse_phases_execute(
) {
    // Arrange
    let cases = [
        (
            "SELECT LOWER(label) FILTER (WHERE TRUE) FROM modifier_values",
            "42809",
        ),
        ("SELECT LOWER(DISTINCT label) FROM modifier_values", "42809"),
        ("SELECT CURRENT_DATABASE() FILTER (WHERE TRUE)", "42809"),
        (
            "SELECT modifier_private.bool_and(amount) FILTER (WHERE TRUE) FROM modifier_values",
            "42809",
        ),
        (
            "SELECT modifier_private.bool_and(DISTINCT amount) FROM modifier_values",
            "42809",
        ),
        (
            "SELECT SUM(amount) FILTER (WHERE CAST(NULL AS INT)) FROM modifier_values",
            "42804",
        ),
        (
            "SELECT SUM(DISTINCT amount ORDER BY label) FROM modifier_values",
            "42P10",
        ),
        (
            "SELECT COUNT(* ORDER BY amount) FROM modifier_values",
            "42601",
        ),
    ];
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE SCHEMA modifier_private")]),
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE FUNCTION modifier_private.bool_and(x INT) RETURNS INT AS 'x'")]),
        Step::ThroughReady(vec![wire::simple_query_frame("CREATE TABLE modifier_values (amount INT, flag BOOLEAN, included BOOLEAN, label TEXT)")]),
        Step::ThroughReady(vec![wire::simple_query_frame("INSERT INTO modifier_values VALUES (1,TRUE,TRUE,'a'), (1,FALSE,FALSE,'b'), (2,NULL,TRUE,'c'), (2,TRUE,NULL,'d')")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT BOOL_AND(flag) FILTER (WHERE included), BOOL_OR(DISTINCT flag), COUNT(DISTINCT amount), SUM(ALL amount) FROM modifier_values")]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT LOWER(ALL 'X')")]),
    ];
    for (sql, _) in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types("", sql, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("issue-804-modifier-value-and-phase-matrix", steps);

    // Assert
    for setup in &batches[..4] {
        assert_eq!(wire::error_code(setup), None);
    }
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(
        wire::data_rows(&batches[4]),
        vec![vec![
            Some("t".into()),
            Some("t".into()),
            Some("2".into()),
            Some("6".into())
        ]]
    );
    let description = batches[4]
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("modifier result metadata");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 20, 20]
    );
    assert_eq!(wire::error_code(&batches[5]), None);
    assert_eq!(wire::data_rows(&batches[5]), vec![vec![Some("x".into())]]);
    for ((sql, expected), phases) in cases.iter().zip(batches[6..].chunks_exact(4)) {
        for error in &phases[..2] {
            assert_eq!(wire::error_code(error).as_deref(), Some(*expected), "{sql}");
            assert!(wire::data_rows(error).is_empty());
            assert!(!error.iter().any(|frame| frame.0 == b'C' || frame.0 == b'1'));
        }
        assert_eq!(phases[2].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&phases[3]), None);
        assert_eq!(wire::data_rows(&phases[3]), vec![vec![Some("42".into())]]);
    }
}

#[test]
fn should_reject_invalid_aggregate_types_given_empty_tables_and_false_filters_when_calls_are_bound()
{
    // Arrange
    let cases = [
        "SELECT SUM(label) FILTER (WHERE FALSE) FROM modifier_empty_types",
        "SELECT AVG(label) FROM modifier_empty_types",
        "SELECT MIN(flag) FILTER (WHERE FALSE) FROM modifier_empty_types",
        "SELECT MAX(flag) FILTER (WHERE NULL) FROM modifier_empty_types",
        "SELECT SUM(DISTINCT label ORDER BY label) FILTER (WHERE FALSE) FROM modifier_empty_types",
        "SELECT COUNT(DISTINCT body) FILTER (WHERE FALSE) FROM modifier_empty_types",
    ];
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE modifier_empty_types (label TEXT, flag BOOLEAN, body JSON)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT COUNT(label), COUNT(body), BOOL_AND(flag) FROM modifier_empty_types",
        )]),
    ];
    for sql in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types("", sql, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("issue-804-empty-type-resolution", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("0".into()), Some("0".into()), None]]
    );
    for (sql, phases) in cases.iter().zip(batches[2..].chunks_exact(4)) {
        for error in &phases[..2] {
            assert_eq!(wire::error_code(error).as_deref(), Some("42883"), "{sql}");
            assert!(wire::data_rows(error).is_empty());
            assert!(!error.iter().any(|frame| frame.0 == b'C' || frame.0 == b'1'));
        }
        assert_eq!(phases[2].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&phases[3]), None);
        assert_eq!(wire::data_rows(&phases[3]), vec![vec![Some("42".into())]]);
    }
}
