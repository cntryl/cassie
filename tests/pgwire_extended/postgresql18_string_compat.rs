//! PostgreSQL 18 string-function parameter and result metadata tracked by #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_bind_split_part_parameters_with_text_descriptor() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "split_part_parameters",
            "SELECT split_part($1, $2, $3)",
            &[25, 25, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("split_part_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "split_part_portal",
                "split_part_parameters",
                &["one,two,three", ",", "2"],
            ),
            wire::describe_portal_frame("split_part_portal"),
            wire::execute_frame("split_part_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-split-part-wire", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![25, 25, 23]
    );
    let result_columns = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_columns.1)[0].type_oid,
        25
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("two".to_owned())]]
    );
}
