//! Literal homogeneous join parity with actual supported direct-kernel counters.
use crate::support_relational_qualification::{
    cycle, fixture_with_vectorized_joins, matches_row, run_wire, Fixture, Parameter,
};
use serde::Deserialize;
#[derive(Deserialize)]
struct Records {
    seeds: Vec<Seed>,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Seed {
    create: String,
    insert: String,
    bind_rows: Vec<Vec<Parameter>>,
}
#[derive(Deserialize)]
struct Case {
    invariant: String,
    sql: String,
    unaliased_sql: String,
    enabled_strategy: String,
    enabled_right_input: u64,
    expected_rows: Vec<Vec<serde_json::Value>>,
}
fn records() -> Records {
    serde_json::from_str(include_str!(
        "../support/relational_qualification/homogeneous_join_records.json"
    ))
    .expect("independent homogeneous pair records")
}
fn seed(fixture: &Fixture, records: &Records) {
    for seed in &records.seeds {
        fixture
            .cassie
            .execute_sql(&fixture.session, &seed.create, vec![])
            .expect("homogeneous table");
    }
    let cycles = records
        .seeds
        .iter()
        .flat_map(|seed| seed.bind_rows.iter().map(|row| cycle(&seed.insert, row, 0)))
        .collect();
    let packets = run_wire(fixture, cycles);
    assert_eq!(packets.len(), 10);
    for frames in packets {
        assert_eq!(crate::support_pgwire::error_code(&frames), None);
    }
}

#[derive(Deserialize)]
struct Counters {
    last_strategy: String,
    vectorized_build_rows_total: u64,
    vectorized_probe_rows_total: u64,
    matched_rows_total: u64,
    left_input_rows_total: u64,
    right_input_rows_total: u64,
    output_rows_total: u64,
}

fn counters(fixture: &Fixture) -> Counters {
    serde_json::from_value(fixture.cassie.metrics()["joins"].clone())
        .expect("existing public join metric fields")
}

#[test]
fn should_match_homogeneous_join_literals_with_selected_kernel_counters() {
    // Arrange
    let records = records();
    for enabled in [false, true] {
        let fixture = fixture_with_vectorized_joins(enabled);
        seed(&fixture, &records);
        for case in &records.cases {
            let before = counters(&fixture);

            // Act
            let direct = fixture
                .cassie
                .execute_sql(&fixture.session, &case.unaliased_sql, vec![])
                .expect("direct homogeneous join");
            let after = counters(&fixture);
            let aliased = fixture
                .cassie
                .execute_sql(&fixture.session, &case.sql, vec![])
                .expect("ordinary alias homogeneous join");

            // Assert
            for result in [direct, aliased] {
                assert_eq!(result.rows.len(), case.expected_rows.len());
                assert!(
                    result
                        .rows
                        .iter()
                        .zip(&case.expected_rows)
                        .all(|(actual, expected)| matches_row(actual, expected)),
                    "{} enabled{enabled}",
                    case.invariant
                );
                assert!(result.columns.iter().all(|column| column.type_oid == 20
                    && column.typlen == 8
                    && column.atttypmod == -1));
            }
            println!(
                "homogeneous {} enabled{enabled} direct_strategy={} build={} probe={} matched={} left={} right={} output={}",
                case.invariant,
                after.last_strategy,
                after.vectorized_build_rows_total - before.vectorized_build_rows_total,
                after.vectorized_probe_rows_total - before.vectorized_probe_rows_total,
                after.matched_rows_total - before.matched_rows_total,
                after.left_input_rows_total - before.left_input_rows_total,
                after.right_input_rows_total - before.right_input_rows_total,
                after.output_rows_total - before.output_rows_total
            );
            if enabled {
                assert_eq!(after.last_strategy, case.enabled_strategy);
                assert_eq!(
                    after.vectorized_build_rows_total - before.vectorized_build_rows_total,
                    4
                );
                assert_eq!(
                    after.vectorized_probe_rows_total - before.vectorized_probe_rows_total,
                    5
                );
                assert_eq!(after.matched_rows_total - before.matched_rows_total, 5);
            } else {
                assert_eq!(after.last_strategy, "merge");
                assert_eq!(
                    after.vectorized_build_rows_total - before.vectorized_build_rows_total,
                    0
                );
            }
            assert_eq!(
                after.left_input_rows_total - before.left_input_rows_total,
                5
            );
            assert_eq!(
                after.right_input_rows_total - before.right_input_rows_total,
                if enabled { case.enabled_right_input } else { 5 }
            );
            assert_eq!(
                after.output_rows_total - before.output_rows_total,
                u64::try_from(case.expected_rows.len()).expect("finite row count")
            );
        }
    }
}

#[test]
fn should_describe_homogeneous_join_alias_outputs_in_both_formats() {
    // Arrange
    let records = records();
    let fixture = fixture_with_vectorized_joins(true);
    seed(&fixture, &records);
    let cycles = records
        .cases
        .iter()
        .flat_map(|case| [cycle(&case.sql, &[], 0), cycle(&case.sql, &[], 1)])
        .collect();

    // Act
    let packets = run_wire(&fixture, cycles);

    // Assert
    assert_eq!(packets.len(), 4);
    for pair in packets.as_chunks::<2>().0 {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(crate::support_pgwire::error_code(frames), None);
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert_eq!(descriptions.len(), 2);
            for (index, description) in descriptions.iter().enumerate() {
                assert_eq!(description.len(), 2);
                for (column, name) in description.iter().zip(["id", "id"]) {
                    assert_eq!(
                        (
                            column.name.as_str(),
                            column.type_oid,
                            column.type_size,
                            column.type_mod,
                            column.format_code
                        ),
                        (name, 20, 8, -1, if index == 0 { 0 } else { format })
                    );
                }
            }
        }
    }
}
