use super::*;

#[test]
fn should_preserve_multidimensional_array_binary_framing_after_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_multidimensional_array_restart (id INT4 PRIMARY KEY, items INT4[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_multidimensional_array_restart_items_idx ON issue_807_multidimensional_array_restart USING btree (items)",
            Vec::new(),
        ),
    ];
    let binary_array = [
        0, 0, 0, 2, // two dimensions
        0, 0, 0, 1, // contains NULL elements
        0, 0, 0, 23, // INT4 element OID
        0, 0, 0, 2, // first dimension length
        0, 0, 0, 0, // first dimension lower bound
        0, 0, 0, 2, // second dimension length
        255, 255, 255, 255, // second dimension lower bound (-1)
        0, 0, 0, 4, 0, 0, 0, 1, // first element
        255, 255, 255, 255, // NULL element
        0, 0, 0, 4, 0, 0, 0, 3, // third element
        0, 0, 0, 4, 0, 0, 0, 4, // fourth element
    ];
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_multidimensional_array_restart VALUES (1, $1)",
        Some((1_007, 1, Some(&binary_array))),
        0,
        false,
    );
    let lookup = fixture::execute_cycle(
        "SELECT items FROM issue_807_multidimensional_array_restart WHERE items = $1 ORDER BY id",
        Some((1_007, 0, Some(b"[0:1][-1:0]={{1,NULL},{3,4}}"))),
        1,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "issue-807-multidimensional-array-binary-restart",
        &setup,
        vec![vec![insert], vec![lookup]],
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

    let result = &stages[1][0];
    fixture::assert_success(result);
    fixture::assert_single_column_descriptors(result, (1_007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&binary_array)]
    );
}
