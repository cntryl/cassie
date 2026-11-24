//! PostgreSQL JSONB key-existence behavior for scalar documents.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_match_text_scalars_but_not_numeric_scalars_for_key_existence() {
    // Arrange
    let sql = "SELECT to_jsonb('foo'::TEXT) ? 'foo' AS matching_text_scalar, \
        to_jsonb('foo'::TEXT) ? 'other' AS different_text_scalar, \
        to_jsonb(1) ? '1' AS numeric_scalar";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-scalar-key-boundary", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB scalar key-existence RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}
