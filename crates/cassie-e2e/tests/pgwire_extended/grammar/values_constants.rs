//! Source constant folding preserves native errors and parameter metadata.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_keep_row_ordering_opaque_given_propagated_fields_when_case_errors_are_planned() {
    // Arrange
    let sql = "SELECT CASE WHEN p THEN TRUE ELSE (1/0)=0 END FROM (VALUES((1,2)<(1,3))) AS v(p)";
    // Act
    let batches = run_script(
        "values-row-opacity",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
}

#[test]
fn should_preserve_integer_field_widths_given_source_constants_when_empty_outputs_are_requested() {
    // Arrange
    let cases = [("SELECT ROW(n+1::SMALLINT)=ROW(0::BIGINT) FROM (VALUES(32767::SMALLINT)) AS v(n) WHERE FALSE", "22003"), ("SELECT 1/b FROM (VALUES(0)) AS v(b) LIMIT 0", "22012")];
    // Act
    let outcomes = cases.map(|(sql, state)| {
        (
            run_script(
                "values-constant-errors",
                vec![
                    Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                    Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
                ],
            ),
            state,
        )
    });
    // Assert
    for (batches, state) in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some(state));
        assert_eq!(
            wire::data_rows(&batches[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[1]), None);
    }
}

#[test]
fn should_retain_supplied_trailing_slots_given_constant_sources_when_prepared_statements_are_described(
) {
    // Arrange
    let batches = run_script(
        "values-constant-parameter-slots",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "p",
                "SELECT $1 AS p FROM (VALUES(1)) AS v(n)",
                &[23, 20],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "out",
                    "p",
                    &[],
                    &[Some(b"7"), Some(b"9007199254740993")],
                    &[1],
                ),
                wire::describe_portal_frame("out"),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    let row = batches[2].iter().find(|frame| frame.0 == b'D');
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("ParameterDescription").1),
        vec![23, 20]
    );
    assert_eq!(&row.expect("binary INT4").1[6..], &7_i32.to_be_bytes());
}

#[test]
fn should_preserve_source_types_given_false_filters_when_prepared_outputs_are_described() {
    // Arrange
    let batches = run_script(
        "values-constant-empty-description",
        vec![
            Step::ParseFlush(wire::parse_frame(
                "p",
                "SELECT n FROM (VALUES(1::SMALLINT)) AS v(n) WHERE FALSE",
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("out", "p", &[], &[], &[]),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let description = batches[1].iter().find(|frame| frame.0 == b'T');
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    let column = &wire::parse_row_description(&description.expect("RowDescription").1)[0];
    assert_eq!(
        (
            column.name.as_str(),
            column.type_oid,
            column.type_size,
            column.type_mod
        ),
        ("n", 21, 2, -1)
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        Vec::<Vec<Option<String>>>::new()
    );
}
