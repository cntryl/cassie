//! Explicit `regconfig` resolution for `ts_headline` over PostgreSQL wire.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_bind_explicit_regconfig_for_ts_headline_over_extended_protocol() {
    // Arrange
    let sql = "SELECT ts_headline($1, $2, to_tsquery($1, $3), $4) AS snippet";
    let batches = run_script(
        "issue-799-ts-headline-regconfig-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "headline_regconfig",
                sql,
                &[3734, 25, 25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("headline_regconfig"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "headline_portal",
                    "headline_regconfig",
                    &[
                        "pg_catalog.english",
                        "The Fat Rats",
                        "rat",
                        "StartSel=<b>, StopSel=</b>",
                    ],
                ),
                wire::describe_portal_frame("headline_portal"),
                wire::execute_frame("headline_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ts_headline ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![3734, 25, 25, 25]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("ts_headline statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)[0].type_oid,
        25
    );
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("ts_headline portal RowDescription");
    let portal_column = wire::parse_row_description(&portal_description.1).remove(0);
    assert_eq!(portal_column.name, "snippet");
    assert_eq!(portal_column.type_oid, 25);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("The Fat <b>Rats</b>".to_owned())]]
    );
}
