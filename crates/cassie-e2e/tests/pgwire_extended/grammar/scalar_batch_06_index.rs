//! Native symmetric bounds remain correct with and without a scalar index.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_symmetric_rows_given_scalar_index_when_bounds_are_reversed() {
    // Arrange
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE symmetric_ranges(v INTEGER)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO symmetric_ranges VALUES (0),(1),(2),(3),(4),(NULL)",
        )]),
    ];
    let queries = [
        "SELECT v+0 AS output FROM symmetric_ranges WHERE v BETWEEN SYMMETRIC 3 AND 1 ORDER BY v",
        "SELECT v+0 AS output FROM symmetric_ranges WHERE v NOT BETWEEN SYMMETRIC 3 AND 1 ORDER BY v",
    ];
    for indexed in [false, true] {
        if indexed {
            steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE INDEX symmetric_ranges_v ON symmetric_ranges(v)",
            )]));
        }
        for query in queries {
            for format in [0, 1] {
                steps.push(Step::ThroughReady(vec![
                    wire::parse_frame("p", query),
                    wire::bind_frame_with_formats("out", "p", &[], &[], &[format]),
                    wire::describe_portal_frame("out"),
                    wire::execute_frame("out"),
                    wire::close_statement_frame("p"),
                    wire::sync_frame(),
                ]));
            }
        }
    }
    // Act
    let batches = run_script("symmetric-index", steps);
    // Assert
    for batch in &batches {
        fixture::assert_success(batch);
    }
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![
            &[0, 1, 0, 0, 0, 1, 49],
            &[0, 1, 0, 0, 0, 1, 50],
            &[0, 1, 0, 0, 0, 1, 51]
        ]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]
        ]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[4]),
        vec![&[0, 1, 0, 0, 0, 1, 48], &[0, 1, 0, 0, 0, 1, 52]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[5]),
        vec![
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]
        ]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[7]),
        vec![
            &[0, 1, 0, 0, 0, 1, 49],
            &[0, 1, 0, 0, 0, 1, 50],
            &[0, 1, 0, 0, 0, 1, 51]
        ]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[8]),
        vec![
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3]
        ]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[9]),
        vec![&[0, 1, 0, 0, 0, 1, 48], &[0, 1, 0, 0, 0, 1, 52]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[10]),
        vec![
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 0],
            &[0, 1, 0, 0, 0, 4, 0, 0, 0, 4]
        ]
    );
}
