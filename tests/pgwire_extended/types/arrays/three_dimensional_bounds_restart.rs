//! PostgreSQL 18 three-dimensional array shape and lower-bound persistence for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_round_trip_three_dimensional_array_state_after_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_3d_restart (id INT4 PRIMARY KEY, items INT4[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_3d_restart_items_idx ON issue_807_array_3d_restart USING btree (items)",
            Vec::new(),
        ),
    ];
    let binary_array = [
        0, 0, 0, 3, // three dimensions
        0, 0, 0, 1, // contains NULL elements
        0, 0, 0, 23, // INT4 element OID
        0, 0, 0, 2, 0, 0, 0, 0, // first dimension: length 2, lower bound 0
        0, 0, 0, 2, 255, 255, 255, 254, // second dimension: length 2, lower bound -2
        0, 0, 0, 2, 0, 0, 0, 3, // third dimension: length 2, lower bound 3
        0, 0, 0, 4, 0, 0, 0, 1, // [0][-2][3]
        255, 255, 255, 255, // [0][-2][4] is SQL NULL
        0, 0, 0, 4, 0, 0, 0, 3, // [0][-1][3]
        0, 0, 0, 4, 0, 0, 0, 4, // [0][-1][4]
        0, 0, 0, 4, 0, 0, 0, 5, // [1][-2][3]
        0, 0, 0, 4, 0, 0, 0, 6, // [1][-2][4]
        0, 0, 0, 4, 0, 0, 0, 7, // [1][-1][3]
        0, 0, 0, 4, 0, 0, 0, 8, // [1][-1][4]
    ];
    let text_array = b"[0:1][-2:-1][3:4]={{{1,NULL},{3,4}},{{5,6},{7,8}}}";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_3d_restart VALUES (1, $1)",
        Some((1007, 1, Some(&binary_array))),
        0,
        false,
    );
    let dimensions = fixture::execute_cycle(
        "SELECT array_dims(items), array_ndims(items), \
                array_lower(items, 1), array_upper(items, 1), \
                array_lower(items, 2), array_upper(items, 2), \
                array_lower(items, 3), array_upper(items, 3), \
                cardinality(items), items \
         FROM issue_807_array_3d_restart WHERE items = $1::INT4[] ORDER BY id",
        Some((1007, 0, Some(text_array))),
        0,
        true,
    );
    let binary_readback = fixture::execute_cycle(
        "SELECT items FROM issue_807_array_3d_restart WHERE id = 1",
        None,
        1,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "issue-807-three-dimensional-array-bounds-restart",
        &setup,
        vec![vec![insert], vec![dimensions, binary_readback]],
    );

    // Assert
    let inserted = &stages[0][0];
    fixture::assert_success(inserted);
    assert_eq!(
        inserted
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );

    let dimensions = &stages[1][0];
    fixture::assert_success(dimensions);
    assert_three_dimensional_array_values(dimensions);

    let binary_readback = &stages[1][1];
    fixture::assert_success(binary_readback);
    assert_three_dimensional_array_binary(binary_readback, &binary_array);
}

fn assert_three_dimensional_array_values(frames: &fixture::Frames) {
    let descriptor = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("three-dimensional array RowDescription");
    assert_eq!(
        wire::parse_row_description(&descriptor.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![
            (25, -1, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (23, 4, -1),
            (1007, -1, -1),
        ]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0:1][-2:-1][3:4]".to_owned()),
            Some("3".to_owned()),
            Some("0".to_owned()),
            Some("1".to_owned()),
            Some("-2".to_owned()),
            Some("-1".to_owned()),
            Some("3".to_owned()),
            Some("4".to_owned()),
            Some("8".to_owned()),
            Some("[0:1][-2:-1][3:4]={{{1,NULL},{3,4}},{{5,6},{7,8}}}".to_owned()),
        ]]
    );
}

fn assert_three_dimensional_array_binary(frames: &fixture::Frames, expected: &[u8]) {
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(expected)]
    );
}
