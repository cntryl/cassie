//! Undefined network operator diagnostics with a supported PostgreSQL control (#816).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_operator_errors_given_supported_network_values_when_undefined_overloads_are_used()
{
    // Arrange
    let control_sql = "SELECT CAST('08:00:2b:01:02:03' AS MACADDR) = \
                              CAST('08:00:2b:01:02:03' AS MACADDR) AS same_address";
    let undefined_sql = "SELECT CAST('08:00:2b:01:02:03' AS MACADDR) + \
                                CAST('08:00:2b:01:02:04' AS MACADDR)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(control_sql)]),
        Step::ThroughReady(vec![wire::simple_query_frame(undefined_sql)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 48 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-816-macaddr-undefined-operator", steps);

    // Assert
    assert_eq!(batches.len(), 3);
    let control = &batches[0];
    assert_eq!(
        wire::error_code(control),
        None,
        "supported MACADDR equality control ErrorResponse fields: {:?}",
        control
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = control
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("MACADDR equality control RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(control), vec![vec![Some("t".to_owned())]]);

    let rejected = &batches[1];
    assert_eq!(
        wire::error_code(rejected).as_deref(),
        Some("42883"),
        "undefined MACADDR operator ErrorResponse fields: {:?}",
        rejected
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert!(rejected
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
        .iter()
        .any(|(kind, message)| {
            *kind == 'M' && message == "operator does not exist: macaddr + macaddr"
        }));
    assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
    assert_eq!(rejected.iter().filter(|(tag, _)| *tag == b'C').count(), 0);

    let recovery = &batches[2];
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("48".to_owned())]]);
}
