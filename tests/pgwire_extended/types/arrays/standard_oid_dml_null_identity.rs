//! Standard array OIDs, SQL NULLs, and null elements through binary DML (#807).

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

const SMALLINT_ARRAY_WITH_NULL: &[u8] = &[
    0, 0, 0, 1, // one dimension
    0, 0, 0, 1, // contains a SQL NULL element
    0, 0, 0, 21, // int2 element OID
    0, 0, 0, 3, // three elements
    0, 0, 0, 1, // lower bound
    0, 0, 0, 2, 0, 2, // int2 value 2
    255, 255, 255, 255, // SQL NULL element
    0, 0, 0, 2, 255, 253, // int2 value -3
];

const EMPTY_BIGINT_ARRAY: &[u8] = &[
    0, 0, 0, 0, // zero dimensions
    0, 0, 0, 0, // no NULL elements
    0, 0, 0, 20, // int8 element OID
];

fn data_row(fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut row = u16::try_from(fields.len())
        .expect("array result field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        match field {
            Some(value) => {
                row.extend_from_slice(
                    &i32::try_from(value.len())
                        .expect("array result field length")
                        .to_be_bytes(),
                );
                row.extend_from_slice(value);
            }
            None => row.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    row
}

#[test]
fn should_preserve_standard_array_wire_identity_through_binary_dml() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_807_array_null_identity (\
            small_values SMALLINT[], integer_values INTEGER[], bigint_values BIGINT[]\
        )",
        Vec::new(),
    )];
    let insert = vec![
        wire::parse_frame_with_types(
            "array_null_identity_insert",
            "INSERT INTO issue_807_array_null_identity \
             VALUES ($1, $2, $3)",
            &[1005, 1007, 1016],
        ),
        wire::bind_frame_with_formats(
            "array_null_identity_portal",
            "array_null_identity_insert",
            &[1, 1, 1],
            &[
                Some(SMALLINT_ARRAY_WITH_NULL),
                None,
                Some(EMPTY_BIGINT_ARRAY),
            ],
            &[],
        ),
        wire::execute_frame("array_null_identity_portal"),
        wire::sync_frame(),
    ];
    let text_readback = fixture::execute_cycle(
        "SELECT small_values, integer_values, bigint_values \
         FROM issue_807_array_null_identity",
        None,
        0,
        true,
    );
    let binary_readback = fixture::execute_cycle(
        "SELECT small_values, integer_values, bigint_values \
         FROM issue_807_array_null_identity",
        None,
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-807-standard-array-dml-null-identity",
        &setup,
        vec![insert, text_readback, binary_readback],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
    for (result_format, frames) in [0, 1].into_iter().zip(&batches[1..]) {
        fixture::assert_success(frames);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("standard array result RowDescription");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(
            columns
                .iter()
                .map(|column| (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                ))
                .collect::<Vec<_>>(),
            vec![
                (1005, -1, -1, result_format),
                (1007, -1, -1, result_format),
                (1016, -1, -1, result_format),
            ]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![
            Some("{2,NULL,-3}".to_owned()),
            None,
            Some("{}".to_owned()),
        ]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![data_row(&[
            Some(SMALLINT_ARRAY_WITH_NULL),
            None,
            Some(EMPTY_BIGINT_ARRAY),
        ])]
    );
}
