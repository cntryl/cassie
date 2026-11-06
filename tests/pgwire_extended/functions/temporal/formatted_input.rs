//! Formatted timestamp range errors preserve prepared descriptors and recovery.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_recover_prepared_timestamp_after_formatted_input_range_errors() {
    // Arrange
    let statement = "formatted_timestamp";
    let template = "YYYY-MM-DD HH24:MI:SS.US TZH";
    let invalid = [
        "2000-01-01 24:00:00.000000 +00",
        "2000-01-01 23:59:60.000000 +00",
        "294276-12-31 23:59:59.000000 -15",
    ];
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT to_timestamp($1, $2)",
            &[25, 25],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
    ];
    for input in invalid {
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame("invalid", statement, &[input, template]),
            wire::execute_frame("invalid"),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(
                "valid",
                statement,
                &["2000-01-01 23:59:59.999999 +00", template],
            ),
            wire::describe_portal_frame("valid"),
            wire::execute_frame("valid"),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("formatted-timestamp-range-recovery", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("statement description");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1184
    );
    assert_eq!(batches.len(), 3 + 2 * invalid.len());
    for pair in batches[3..].as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("22008"));
        assert!(!pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(pair[0].last().map(|(tag, _)| *tag), Some(b'Z'));
        assert_eq!(wire::error_code(&pair[1]), None);
        let description = pair[1]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("portal description");
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            1184
        );
        assert_eq!(
            wire::data_rows(&pair[1]),
            vec![vec![Some("2000-01-01 23:59:59.999999+00".to_owned())]]
        );
    }
}
