//! PostgreSQL 18 array subquery construction tracked by #807.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

fn array_subquery_setup() -> [(&'static str, Vec<Value>); 2] {
    [
        (
            "CREATE TABLE issue_807_array_subquery_input (ordinal INT4 PRIMARY KEY, item INT4)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_subquery_input VALUES (2, NULL), (1, 10), (3, 30)",
            Vec::new(),
        ),
    ]
}

fn assert_array_descriptor(frames: &fixture::Frames, format: i16) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for (index, columns) in descriptions.iter().enumerate() {
        let expected_format = if index == 0 { 0 } else { format };
        assert_eq!(columns.len(), 1);
        let column = &columns[0];
        assert_eq!(
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ),
            (1_007, -1, -1, expected_format)
        );
    }
}

fn binary_data_row(field: &[u8]) -> Vec<u8> {
    let mut row = 1_i16.to_be_bytes().to_vec();
    row.extend_from_slice(
        &i32::try_from(field.len())
            .expect("array binary field length")
            .to_be_bytes(),
    );
    row.extend_from_slice(field);
    row
}

#[test]
fn should_construct_array_from_ordered_subquery_with_null_elements() {
    // Arrange
    let sql =
        "SELECT ARRAY(SELECT item FROM issue_807_array_subquery_input ORDER BY ordinal) AS value";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-ordered-subquery",
        &array_subquery_setup(),
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_array_descriptor(&frames, format);
        if format == 0 {
            assert_eq!(
                wire::data_rows(&frames),
                vec![vec![Some("{10,NULL,30}".to_owned())]]
            );
        } else {
            let populated = [
                0, 0, 0, 1, // dimensions
                0, 0, 0, 1, // has NULL elements
                0, 0, 0, 23, // INT4 element OID
                0, 0, 0, 3, // element count
                0, 0, 0, 1, // lower bound
                0, 0, 0, 4, 0, 0, 0, 10, // first element
                255, 255, 255, 255, // SQL NULL element
                0, 0, 0, 4, 0, 0, 0, 30, // last element
            ];
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![binary_data_row(&populated)]
            );
        }
    }
}

#[test]
fn should_return_typed_empty_array_for_zero_row_subquery() {
    // Arrange
    let sql =
        "SELECT ARRAY(SELECT item FROM issue_807_array_subquery_input WHERE ordinal = 99) AS value";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-empty-subquery",
        &array_subquery_setup(),
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_array_descriptor(&frames, format);
        if format == 0 {
            assert_eq!(wire::data_rows(&frames), vec![vec![Some("{}".to_owned())]]);
        } else {
            let empty = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23];
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![binary_data_row(&empty)]
            );
        }
    }
}
