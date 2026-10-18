//! `range_merge(anymultirange)` output values and identities for #905.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_merge_each_builtin_multirange_to_its_range_hull() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-range-merge", &["SET TIME ZONE 'UTC'"]);
    let query = "SELECT \
         range_merge('{[1,3),[5,7)}'::INT4MULTIRANGE), \
         range_merge('{[1,3),[5,7)}'::INT8MULTIRANGE), \
         range_merge('{[1.25,2.50),[5,6)}'::NUMMULTIRANGE), \
         range_merge('{[2024-01-01,2024-01-03),[2024-01-05,2024-01-07)}'::DATEMULTIRANGE), \
         range_merge('{[2024-01-01 00:00:00,2024-01-03 00:00:00),[2024-01-05 00:00:00,2024-01-07 00:00:00)}'::TSMULTIRANGE), \
         range_merge('{[2024-01-01 00:00:00+00,2024-01-03 00:00:00+00),[2024-01-05 00:00:00+00,2024-01-07 00:00:00+00)}'::TSTZMULTIRANGE)";

    // Act
    let result = fixture.execute(query);

    // Assert
    let result = result.expect("range_merge accepts every built-in multirange family");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3904, 3926, 3906, 3912, 3908, 3910]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("[1,7)".to_owned()),
            Value::String("[1,7)".to_owned()),
            Value::String("[1.25,6)".to_owned()),
            Value::String("[2024-01-01,2024-01-07)".to_owned()),
            Value::String("[\"2024-01-01 00:00:00\",\"2024-01-07 00:00:00\")".to_owned()),
            Value::String("[\"2024-01-01 00:00:00+00\",\"2024-01-07 00:00:00+00\")".to_owned(),),
        ]]
    );
}
