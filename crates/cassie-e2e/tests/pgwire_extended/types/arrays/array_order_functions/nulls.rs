use super::*;

fn run_array_query(sql: &str, label: &str) -> Vec<(u8, Vec<u8>)> {
    let cycle = fixture::execute_cycle(sql, None, 0, true);
    fixture::run_cycles(label, &[], vec![cycle]).remove(0)
}

#[test]
fn should_sort_array_elements_with_nulls_last() {
    // Arrange
    let sql = "SELECT array_sort(ARRAY[3,1,NULL,2]) AS sorted";

    // Act
    let frames = run_array_query(sql, "issue-807-array-sort-nulls-last");

    // Assert
    fixture::assert_success(&frames);
    fixture::assert_single_column_descriptors(&frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("{1,2,3,NULL}".to_owned())]]
    );
}

include!("semantics.rs");
