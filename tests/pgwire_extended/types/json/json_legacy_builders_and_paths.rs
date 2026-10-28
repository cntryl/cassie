//! Legacy JSON constructors and path function overloads over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_extract_nested_values_through_json_path_function_variants() {
    // Arrange
    let sql = r#"SELECT
        json_extract_path('{"nested":{"name":"Ada","value":null}}'::JSON, 'nested', 'name'),
        jsonb_extract_path('{"nested":{"name":"Ada","value":null}}'::JSONB, 'nested', 'name'),
        json_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSON, 'nested', 'name'),
        jsonb_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSONB, 'nested', 'name'),
        json_extract_path('{"nested":{"name":"Ada","value":null}}'::JSON, 'missing'),
        jsonb_extract_path('{"nested":{"name":"Ada","value":null}}'::JSONB, 'missing'),
        json_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSON, 'missing'),
        jsonb_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSONB, 'missing'),
        json_extract_path('{"nested":{"name":"Ada","value":null}}'::JSON, 'nested', 'value'),
        jsonb_extract_path('{"nested":{"name":"Ada","value":null}}'::JSONB, 'nested', 'value'),
        json_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSON, 'nested', 'value'),
        jsonb_extract_path_text('{"nested":{"name":"Ada","value":null}}'::JSONB, 'nested', 'value')"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-path-overload-null-semantics",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_type_oids(frames),
        vec![114, 3_802, 25, 25, 114, 3_802, 25, 25, 114, 3_802, 25, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("\"Ada\"".to_owned()),
            Some("\"Ada\"".to_owned()),
            Some("Ada".to_owned()),
            Some("Ada".to_owned()),
            None,
            None,
            None,
            None,
            Some("null".to_owned()),
            Some("null".to_owned()),
            None,
            None,
        ]]
    );
}
