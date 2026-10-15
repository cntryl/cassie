//! SQL/JSON serialization result values and declared PostgreSQL types.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_declared_varchar_type_for_json_serialization() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_json_serialize_values (id INTEGER, document JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_json_serialize_values VALUES (1, '{\"n\":1,\"flag\":true}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT JSON_SERIALIZE(document), JSON_SERIALIZE(document RETURNING VARCHAR) FROM issue_810_json_serialize_values ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-serialize-result-types", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_SERIALIZE RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(25, -1, -1), (1043, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"n":1,"flag":true}"#.to_owned()),
            Some(r#"{"n":1,"flag":true}"#.to_owned()),
        ]]
    );
}
