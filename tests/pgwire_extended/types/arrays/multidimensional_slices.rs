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
fn should_slice_multidimensional_arrays_with_postgresql_wire_identity() {
    // Arrange
    let sql = "SELECT (ARRAY[[10,NULL,30],[40,50,60]])[1:2][1:2] AS selected, \
                      array_dims((ARRAY[[10,NULL,30],[40,50,60]])[1:2][1:2]) AS dimensions, \
                      (ARRAY[[10,NULL,30],[40,50,60]])[1][1] AS in_range, \
                      (ARRAY[[10,NULL,30],[40,50,60]])[3][1] AS out_of_range";
    let cycles = [0, 1]
        .into_iter()
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-multidimensional-array-slices", &[], cycles);

    // Assert
    let binary_slice = [
        0, 0, 0, 2, // dimensions
        0, 0, 0, 1, // has NULL elements
        0, 0, 0, 23, // INT4 element OID
        0, 0, 0, 2, 0, 0, 0, 1, // first dimension: length 2, lower bound 1
        0, 0, 0, 2, 0, 0, 0, 1, // second dimension: length 2, lower bound 1
        0, 0, 0, 4, 0, 0, 0, 10, // first element
        255, 255, 255, 255, // SQL NULL element
        0, 0, 0, 4, 0, 0, 0, 40, // third element
        0, 0, 0, 4, 0, 0, 0, 50, // fourth element
    ];
    let forty = 40_i32.to_be_bytes();
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("multidimensional slice RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| {
                    (
                        column.type_oid,
                        column.type_size,
                        column.type_mod,
                        column.format_code,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (1_007, -1, -1, format),
                (25, -1, -1, format),
                (23, 4, -1, format),
                (23, 4, -1, format),
            ]
        );
        if format == 0 {
            assert_eq!(
                wire::data_rows(&frames),
                vec![vec![
                    Some("{{10,NULL},{40,50}}".to_owned()),
                    Some("[1:2][1:2]".to_owned()),
                    Some("40".to_owned()),
                    None,
                ]]
            );
        } else {
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![binary_data_row(&[
                    Some(&binary_slice),
                    Some(b"[1:2][1:2]"),
                    Some(&forty),
                    None,
                ])]
            );
        }
    }
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
