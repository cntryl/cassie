//! Parameter identity for the private quantified pattern lowering.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_quantified_pattern_text_array_parameters() {
    // Arrange
    let batches = run_script(
        "issue-799-quantified-pattern-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "quantified",
                "SELECT $1 ILIKE ANY($2) AS matched",
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("quantified"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "quantified_portal",
                    "quantified",
                    &["ABC", r#"{"z%","a%"}"#],
                ),
                wire::describe_portal_frame("quantified_portal"),
                wire::execute_frame("quantified_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    let parameters = &described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription")
        .1;
    assert_eq!(
        wire::parse_parameter_description(parameters),
        vec![25, 1009]
    );
    for batch in [described, executed] {
        let columns = wire::parse_row_description(
            &batch
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("RowDescription")
                .1,
        );
        assert_eq!(columns[0].type_oid, 16);
    }
    assert_eq!(wire::data_rows(executed), vec![vec![Some("t".into())]]);
}
