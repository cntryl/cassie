//! JSON and JSONB inspection semantics for SQL NULL and document values.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_classify_json_documents_without_collapsing_sql_nulls() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_json_type_null_cases (id INTEGER, plain_json JSON, binary_json JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_json_type_null_cases VALUES (1, NULL, NULL), (2, 'null', 'null'), (3, 'true', 'true'), (4, '[null]', '[null]'), (5, '{\"n\":1}', '{\"n\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT id, json_typeof(plain_json), jsonb_typeof(binary_json) FROM issue_810_json_type_null_cases ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-type-null-semantics", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON inspection RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), None, None],
            vec![
                Some("2".to_owned()),
                Some("null".to_owned()),
                Some("null".to_owned())
            ],
            vec![
                Some("3".to_owned()),
                Some("boolean".to_owned()),
                Some("boolean".to_owned())
            ],
            vec![
                Some("4".to_owned()),
                Some("array".to_owned()),
                Some("array".to_owned())
            ],
            vec![
                Some("5".to_owned()),
                Some("object".to_owned()),
                Some("object".to_owned())
            ],
        ]
    );
}
