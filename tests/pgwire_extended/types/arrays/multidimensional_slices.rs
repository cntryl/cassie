//! Multidimensional array slices retain PostgreSQL bounds and wire identity (#807).

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

fn binary_data_row(fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut row = i16::try_from(fields.len())
        .expect("array slice result field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        match field {
            Some(bytes) => {
                row.extend_from_slice(
                    &i32::try_from(bytes.len())
                        .expect("array slice result field length")
                        .to_be_bytes(),
                );
                row.extend_from_slice(bytes);
            }
            None => row.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    row
}

#[test]
fn should_preserve_nondefault_bounds_in_int4_array_text_bind() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT $1::INT4[] AS items",
        Some((1_007, 0, Some(b"[0:2]={10,20,30}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-lower-bound-text-bind", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("non-default-bound int4[] RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1_007, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:2]={10,20,30}".to_owned())]]
    );
}
