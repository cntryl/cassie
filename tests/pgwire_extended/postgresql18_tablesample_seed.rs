//! PostgreSQL 18 TABLESAMPLE repeatability through extended pgwire, tracked by #800.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

fn rows_for_sampled_ids(ids: &[i32]) -> Vec<Vec<Option<String>>> {
    ids.iter().map(|id| vec![Some(id.to_string())]).collect()
}

#[test]
fn should_return_repeatable_bernoulli_seed_samples_over_pgwire() {
    // Arrange
    let sql = "SELECT id FROM issue_800_seed_wire_rows \
               TABLESAMPLE BERNOULLI ($1) REPEATABLE ($2) ORDER BY id";
    let inserted_rows = (1..=32)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO issue_800_seed_wire_rows VALUES {inserted_rows}");
    let batches = run_script(
        "issue-800-repeatable-seed-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_seed_wire_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(&insert)]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "repeatable_sample",
                sql,
                &[700, 701],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("repeatable_sample"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("sample_seed_first", "repeatable_sample", &["50", "0.5"]),
                wire::describe_portal_frame("sample_seed_first"),
                wire::execute_frame("sample_seed_first"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("sample_seed_repeat", "repeatable_sample", &["50", "0.5"]),
                wire::describe_portal_frame("sample_seed_repeat"),
                wire::execute_frame("sample_seed_repeat"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("sample_seed_other", "repeatable_sample", &["50", "1.5"]),
                wire::describe_portal_frame("sample_seed_other"),
                wire::execute_frame("sample_seed_other"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let parsed = &batches[2];
    let described = &batches[3];
    let first = &batches[4];
    let repeated = &batches[5];
    let other_seed = &batches[6];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none())
    );
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("TABLESAMPLE ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![700, 701]
    );
    let columns = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TABLESAMPLE statement RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
    assert_eq!(wire::error_code(first), None);
    assert_eq!(wire::error_code(repeated), None);
    assert_eq!(wire::error_code(other_seed), None);

    // PostgreSQL 18.6's BERNOULLI samples for the one-block 1..=32 fixture.
    let expected_first =
        rows_for_sampled_ids(&[2, 5, 6, 9, 10, 11, 15, 17, 18, 21, 22, 23, 24, 29, 30]);
    let expected_other_seed = rows_for_sampled_ids(&[2, 3, 4, 7, 9, 12, 17, 18, 25, 26, 27, 29]);
    assert_eq!(wire::data_rows(first), expected_first);
    assert_eq!(wire::data_rows(repeated), expected_first);
    assert_eq!(wire::data_rows(other_seed), expected_other_seed);
}

#[test]
fn should_return_repeatable_system_seed_samples_over_pgwire() {
    // Arrange
    let sql = "SELECT id FROM issue_800_system_seed_wire_rows \
               TABLESAMPLE SYSTEM ($1) REPEATABLE ($2) ORDER BY id";
    let inserted_rows = (1..=128)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO issue_800_system_seed_wire_rows VALUES {inserted_rows}");
    let batches = run_script(
        "issue-800-system-repeatable-seed-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_system_seed_wire_rows (id BIGINT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(&insert)]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "system_repeatable_sample",
                sql,
                &[700, 701],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("system_repeatable_sample"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "system_sample_seed_first",
                    "system_repeatable_sample",
                    &["50", "0.5"],
                ),
                wire::describe_portal_frame("system_sample_seed_first"),
                wire::execute_frame("system_sample_seed_first"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "system_sample_seed_repeat",
                    "system_repeatable_sample",
                    &["50", "0.5"],
                ),
                wire::describe_portal_frame("system_sample_seed_repeat"),
                wire::execute_frame("system_sample_seed_repeat"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "system_sample_seed_other",
                    "system_repeatable_sample",
                    &["50", "1.5"],
                ),
                wire::describe_portal_frame("system_sample_seed_other"),
                wire::execute_frame("system_sample_seed_other"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let parsed = &batches[2];
    let described = &batches[3];
    let first = &batches[4];
    let repeated = &batches[5];
    let other_seed = &batches[6];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none())
    );
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("TABLESAMPLE SYSTEM ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![700, 701]
    );
    let columns = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TABLESAMPLE SYSTEM statement RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 20);
    assert_eq!(wire::error_code(first), None);
    assert_eq!(wire::error_code(repeated), None);
    assert_eq!(wire::error_code(other_seed), None);

    // PostgreSQL 18.6 selects the fixture's single heap block for seed 0.5 and
    // no block for seed 1.5 at a 50 percent SYSTEM rate.
    let expected_first = (1..=128)
        .map(|id| vec![Some(id.to_string())])
        .collect::<Vec<_>>();
    assert_eq!(wire::data_rows(first), expected_first);
    assert_eq!(wire::data_rows(repeated), expected_first);
    assert!(wire::data_rows(other_seed).is_empty());
}
