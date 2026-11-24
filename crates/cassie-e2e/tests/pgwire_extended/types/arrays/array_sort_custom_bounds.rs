//! Array sorting must retain PostgreSQL custom lower bounds (#807).

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_sort_custom_bound_arrays_with_null_options() {
    // Arrange
    let input = b"[0:3]={3,NULL,1,2}";
    let cycle = fixture::execute_cycle(
        "SELECT array_sort($1::INT4[])::TEXT, \
                array_sort($1::INT4[], TRUE, TRUE)::TEXT, \
                array_dims(array_sort($1::INT4[])), \
                array_dims(array_sort($1::INT4[], TRUE, TRUE)), \
                pg_typeof(array_sort($1::INT4[]))::OID",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-sort-custom-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array sort RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25, 25, 26]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0:3]={1,2,3,NULL}".to_owned()),
            Some("[0:3]={NULL,3,2,1}".to_owned()),
            Some("[0:3]".to_owned()),
            Some("[0:3]".to_owned()),
            Some("1007".to_owned()),
        ]]
    );
}
