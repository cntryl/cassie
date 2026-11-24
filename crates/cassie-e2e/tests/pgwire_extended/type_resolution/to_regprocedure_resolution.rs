//! Exact `to_regprocedure` overload lookup and missing-name behavior for #818.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_resolve_exact_regprocedure_signature_over_wire() {
    // Arrange
    let sql = "SELECT to_regprocedure('pg_catalog.abs(integer)') AS resolved, \
                     to_regprocedure('pg_catalog.abs(integer)')::oid AS resolved_oid";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-818-to-regprocedure-overload", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "to_regprocedure ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("to_regprocedure RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2202, 26]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("abs(integer)".to_owned()),
            Some("1397".to_owned())
        ]]
    );
}

#[test]
fn should_return_null_for_missing_regprocedure_signature() {
    // Arrange
    let sql = "SELECT to_regprocedure('pg_catalog.not_a_real_function(integer)') \
               IS NULL AS missing_is_null";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-818-missing-to-regprocedure", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "missing to_regprocedure ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("missing to_regprocedure RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}
