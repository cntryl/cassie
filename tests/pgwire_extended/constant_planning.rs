//! Planning-time constant errors and EXISTS target elision across simple and
//! extended text/binary execution.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

const PLANNING_ERRORS: [&str; 2] = [
    "UPDATE rt SET id=id RETURNING id, CASE WHEN id<0 THEN 1/0 ELSE 1 END AS selected",
    "UPDATE rt SET id=id+10 WHERE id<0 RETURNING 1/0 AS selected",
];
const EXISTS_TARGET: &str = "SELECT EXISTS(SELECT e.n/e.n FROM ri e) AS selected";

fn setup() -> Vec<(&'static str, Vec<Value>)> {
    [
        "CREATE TABLE rt (id BIGINT)",
        "INSERT INTO rt VALUES (1), (2)",
        "CREATE TABLE ri (n BIGINT)",
        "INSERT INTO ri VALUES (0)",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect()
}

fn modes(sql: &str) -> Vec<Vec<Vec<u8>>> {
    vec![
        vec![wire::simple_query_frame(sql)],
        fixture::execute_cycle(sql, None, 0, true),
        fixture::execute_cycle(sql, None, 1, true),
    ]
}

#[test]
fn should_report_planning_time_division_by_zero_in_every_protocol_mode() {
    // Arrange
    let mut cycles: Vec<_> = PLANNING_ERRORS.iter().flat_map(|sql| modes(sql)).collect();
    cycles.push(vec![wire::simple_query_frame(
        "SELECT id FROM rt ORDER BY id",
    )]);

    // Act
    let batches = fixture::run_cycles("constant-planning-wire", &setup(), cycles);

    // Assert
    let (readback, errors) = batches.split_last().expect("readback batch");
    for frames in errors {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22012"));
        assert!(frames.iter().all(|(tag, _)| !matches!(tag, b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::data_rows(readback),
        vec![vec![Some("1".to_string())], vec![Some("2".to_string())]]
    );
}

#[test]
fn should_return_exists_without_target_evaluation_in_every_protocol_mode() {
    // Arrange
    let cycles = modes(EXISTS_TARGET);

    // Act
    let batches = fixture::run_cycles("exists-target-elision-wire", &setup(), cycles);

    // Assert
    let [simple, text, binary] = batches.as_slice() else {
        panic!("three protocol modes");
    };
    for frames in [simple, text, binary] {
        fixture::assert_success(frames);
    }
    assert_eq!(wire::data_rows(simple), vec![vec![Some("t".to_string())]]);
    fixture::assert_single_column_descriptors(text, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(text), vec![vec![Some("t".to_string())]]);
    fixture::assert_single_column_descriptors(binary, (16, 1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(binary),
        vec![fixture::one_field_row(&[1])]
    );
}
