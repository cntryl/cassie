//! Random array functions preserve PostgreSQL bounds and element bags (#807).

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn array_bag(array_text: &str) -> Vec<Option<i32>> {
    let literal = array_text
        .split_once('=')
        .map_or(array_text, |(_, value)| value);
    let elements = literal
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .expect("one-dimensional array text");
    let mut bag = if elements.is_empty() {
        Vec::new()
    } else {
        elements
            .split(',')
            .map(|element| {
                if element == "NULL" {
                    None
                } else {
                    Some(element.parse().expect("integer array element"))
                }
            })
            .collect()
    };
    bag.sort_unstable();
    bag
}

#[test]
fn should_preserve_custom_bounds_for_array_randomization() {
    // Arrange
    let input = b"[0:5]={1,NULL,2,NULL,3,4}";
    let cycle = fixture::execute_cycle(
        "SELECT array_shuffle($1::INT4[]), \
                array_dims(array_shuffle($1::INT4[])), \
                array_lower(array_shuffle($1::INT4[]), 1), \
                cardinality(array_shuffle($1::INT4[])), \
                pg_typeof(array_shuffle($1::INT4[]))::OID, \
                array_sample($1::INT4[], 6), \
                array_dims(array_sample($1::INT4[], 6)), \
                array_lower(array_sample($1::INT4[], 6), 1), \
                cardinality(array_sample($1::INT4[], 6)), \
                pg_typeof(array_sample($1::INT4[], 6))::OID",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-randomization-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("randomized array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1_007, 25, 23, 23, 26, 1_007, 25, 23, 23, 26]
    );

    let rows = wire::data_rows(frames);
    let row = rows.first().expect("randomized array result row");
    let expected_bag = vec![None, None, Some(1), Some(2), Some(3), Some(4)];
    assert_eq!(
        array_bag(row[0].as_deref().expect("shuffled array text")),
        expected_bag,
        "array_shuffle retains every NULL and non-NULL input element"
    );
    assert_eq!(row[1].as_deref(), Some("[0:5]"));
    assert_eq!(row[2].as_deref(), Some("0"));
    assert_eq!(row[3].as_deref(), Some("6"));
    assert_eq!(row[4].as_deref(), Some("1007"));

    assert_eq!(
        array_bag(row[5].as_deref().expect("sampled array text")),
        expected_bag,
        "sampling the full cardinality retains the input bag including NULLs"
    );
    assert_eq!(row[6].as_deref(), Some("[1:6]"));
    assert_eq!(row[7].as_deref(), Some("1"));
    assert_eq!(row[8].as_deref(), Some("6"));
    assert_eq!(row[9].as_deref(), Some("1007"));
}
