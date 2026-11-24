//! UUID and `pg_lsn` values across persisted insert, update, and restart.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

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

#[test]
fn should_preserve_uuid_binary_identity_given_inserted_and_updated_rows_when_storage_restarts() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_816_uuid_restart (id INT4 PRIMARY KEY, value UUID)",
        Vec::new(),
    )];
    let initial = b"00000000-0000-0000-0000-000000000001";
    let updated = b"a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
    let insert = mutation_cycle(
        "INSERT INTO issue_816_uuid_restart VALUES (1, $1)",
        2950,
        initial,
    );
    let update = mutation_cycle(
        "UPDATE issue_816_uuid_restart SET value = $1 WHERE id = 1",
        2950,
        updated,
    );
    let text_readback = fixture::execute_cycle(
        "SELECT value FROM issue_816_uuid_restart WHERE id = 1",
        None,
        0,
        true,
    );
    let binary_readback = fixture::execute_cycle(
        "SELECT value FROM issue_816_uuid_restart WHERE id = 1",
        None,
        1,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "issue-816-uuid-insert-update-restart",
        &setup,
        vec![vec![insert, update], vec![text_readback, binary_readback]],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_mutation(&stages[0][0], b"INSERT 0 1\0");
    assert_mutation(&stages[0][1], b"UPDATE 1\0");
    let text = &stages[1][0];
    fixture::assert_success(text);
    fixture::assert_single_column_descriptors(text, (2950, 16, -1), 0, true);
    assert_eq!(
        wire::data_rows(text),
        vec![vec![Some(
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".to_owned()
        )]]
    );

    let binary = &stages[1][1];
    fixture::assert_success(binary);
    fixture::assert_single_column_descriptors(binary, (2950, 16, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(binary),
        vec![binary_row(&[
            0xa0, 0xee, 0xbc, 0x99, 0x9c, 0x0b, 0x4e, 0xf8, 0xbb, 0x6d, 0x6b, 0xb9, 0xbd, 0x38,
            0x0a, 0x11,
        ])]
    );
}

#[test]
fn should_preserve_pg_lsn_binary_identity_given_inserted_and_updated_rows_when_storage_restarts() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_816_lsn_restart (id INT4 PRIMARY KEY, value PG_LSN)",
        Vec::new(),
    )];
    let insert = mutation_cycle(
        "INSERT INTO issue_816_lsn_restart VALUES (1, $1)",
        3220,
        b"0/1",
    );
    let update = mutation_cycle(
        "UPDATE issue_816_lsn_restart SET value = $1 WHERE id = 1",
        3220,
        b"16/B374D848",
    );
    let text_readback = fixture::execute_cycle(
        "SELECT value FROM issue_816_lsn_restart WHERE id = 1",
        None,
        0,
        true,
    );
    let binary_readback = fixture::execute_cycle(
        "SELECT value FROM issue_816_lsn_restart WHERE id = 1",
        None,
        1,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "issue-816-lsn-insert-update-restart",
        &setup,
        vec![vec![insert, update], vec![text_readback, binary_readback]],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_mutation(&stages[0][0], b"INSERT 0 1\0");
    assert_mutation(&stages[0][1], b"UPDATE 1\0");
    let text = &stages[1][0];
    fixture::assert_success(text);
    fixture::assert_single_column_descriptors(text, (3220, 8, -1), 0, true);
    assert_eq!(
        wire::data_rows(text),
        vec![vec![Some("16/B374D848".to_owned())]]
    );

    let binary = &stages[1][1];
    fixture::assert_success(binary);
    fixture::assert_single_column_descriptors(binary, (3220, 8, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(binary),
        vec![binary_row(&[
            0x00, 0x00, 0x00, 0x16, 0xb3, 0x74, 0xd8, 0x48
        ])]
    );
}

fn mutation_cycle(sql: &str, type_oid: i32, value: &[u8]) -> Vec<Vec<u8>> {
    vec![
        wire::parse_frame_with_types("", sql, &[type_oid]),
        wire::bind_frame_with_formats("", "", &[0], &[Some(value)], &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
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
