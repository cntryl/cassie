//! PostgreSQL 18 bytea utility overload identity tracked by #816.

use super::{row_description, run_script, wire, Step};

#[test]
fn should_describe_bytea_octet_length_result_as_integer() {
    // Arrange
    let sql = "SELECT OCTET_LENGTH(decode('4142', 'hex')) AS octet_length";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-816-bytea-octet-length-result-identity", steps);
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "PostgreSQL 18.6 defines OCTET_LENGTH(bytea) as integer; ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1)]
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("2".to_owned())]]);
}
