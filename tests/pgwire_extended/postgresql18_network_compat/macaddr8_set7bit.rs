use super::*;

#[test]
fn should_set_the_seventh_bit_for_macaddr8_values() {
    // Arrange
    let sql = "SELECT macaddr8_set7bit('00:00:00:00:00:00:00:00'::macaddr8) AS unset, \
                     macaddr8_set7bit('08:00:2b:01:02:03:04:05'::macaddr8) AS shifted, \
                     macaddr8_set7bit('0a:00:2b:01:02:03:04:05'::macaddr8) AS already_set";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-macaddr8-set7bit", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(774, 8); 3]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("02:00:00:00:00:00:00:00".to_owned()),
            Some("0a:00:2b:01:02:03:04:05".to_owned()),
            Some("0a:00:2b:01:02:03:04:05".to_owned()),
        ]]
    );
}
