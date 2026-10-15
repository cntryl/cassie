//! Money values through binary Bind, persisted updates, and restart.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_money_binary_identity_given_inserted_and_updated_rows_when_storage_restarts() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_908_money_restart (id INT4 PRIMARY KEY, amount MONEY)",
        Vec::new(),
    )];
    let initial = 1_234_i64.to_be_bytes();
    let updated = 9_876_i64.to_be_bytes();
    let insert = binary_mutation_cycle(
        "INSERT INTO issue_908_money_restart VALUES (1, $1)",
        &initial,
    );
    let update = binary_mutation_cycle(
        "UPDATE issue_908_money_restart SET amount = $1 WHERE id = 1",
        &updated,
    );
    let numeric_readback = fixture::execute_cycle(
        "SELECT amount::NUMERIC FROM issue_908_money_restart WHERE id = 1",
        None,
        0,
        true,
    );
    let binary_readback = fixture::execute_cycle(
        "SELECT amount FROM issue_908_money_restart WHERE id = 1",
        None,
        1,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "issue-908-money-insert-update-restart",
        &setup,
        vec![
            vec![insert, update],
            vec![numeric_readback, binary_readback],
        ],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_mutation(&stages[0][0], b"INSERT 0 1\0");
    assert_mutation(&stages[0][1], b"UPDATE 1\0");

    let numeric = &stages[1][0];
    fixture::assert_success(numeric);
    fixture::assert_single_column_descriptors(numeric, (1_700, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(numeric),
        vec![vec![Some("98.76".to_owned())]]
    );

    let binary = &stages[1][1];
    fixture::assert_success(binary);
    fixture::assert_single_column_descriptors(binary, (790, 8, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(binary),
        vec![binary_row(&updated)]
    );
}

fn binary_mutation_cycle(sql: &str, value: &[u8]) -> Vec<Vec<u8>> {
    vec![
        wire::parse_frame_with_types("", sql, &[790]),
        wire::bind_frame_with_formats("", "", &[1], &[Some(value)], &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

fn binary_row(field: &[u8]) -> Vec<u8> {
    let mut payload = vec![0, 1];
    payload.extend_from_slice(
        &i32::try_from(field.len())
            .expect("binary result field length fits i32")
            .to_be_bytes(),
    );
    payload.extend_from_slice(field);
    payload
}

fn assert_mutation(frames: &[(u8, Vec<u8>)], expected_command: &[u8]) {
    fixture::assert_success(frames);
    assert_eq!(
        frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(expected_command)
    );
}
