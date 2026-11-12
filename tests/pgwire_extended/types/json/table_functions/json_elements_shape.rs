//! Ordinality, aliases, LATERAL correlation and bind parameters for the JSON element sources.

use super::json_elements_support::{column_names, column_oids, docs_setup, query, row, text_rows};
use super::{fixture, wire};

#[test]
fn should_append_bigint_ordinality_given_with_ordinality_on_array_elements() {
    // Arrange
    let sql = "SELECT value, ordinality FROM jsonb_array_elements('[5,6]'::jsonb) WITH ORDINALITY";
    // Act
    let frames = query("json-elements-ordinality", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![3802, 20]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![row(&[Some("5"), Some("1")]), row(&[Some("6"), Some("2")])]
    );
}

#[test]
fn should_number_members_in_input_order_given_json_each_with_ordinality() {
    // Arrange
    let sql =
        r#"SELECT key, value, ordinality FROM json_each('{"b":1,"a":2}'::json) WITH ORDINALITY"#;
    // Act
    let frames = query("json-elements-each-ordinality", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![25, 114, 20]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("b"), Some("1"), Some("1")]),
            row(&[Some("a"), Some("2"), Some("2")]),
        ]
    );
}

#[test]
fn should_rename_columns_given_a_column_alias_list_with_ordinality() {
    // Arrange
    let sql = "SELECT v, n FROM jsonb_array_elements('[5,6]'::jsonb) WITH ORDINALITY AS t(v, n)";
    // Act
    let frames = query("json-elements-alias-list", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_names(&frames), vec!["v".to_owned(), "n".to_owned()]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![row(&[Some("5"), Some("1")]), row(&[Some("6"), Some("2")])]
    );
}

#[test]
fn should_keep_default_names_for_unlisted_columns_given_a_partial_alias_list() {
    // Arrange
    let sql = r#"SELECT t.k, t.value FROM jsonb_each('{"a":1}'::jsonb) AS t(k)"#;
    // Act
    let frames = query("json-elements-partial-alias", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        column_names(&frames),
        vec!["k".to_owned(), "value".to_owned()]
    );
    assert_eq!(wire::data_rows(&frames), vec![row(&[Some("a"), Some("1")])]);
}

#[test]
fn should_qualify_columns_by_table_alias_or_function_name() {
    // Arrange
    let aliased =
        "SELECT e.value, e.ordinality FROM jsonb_array_elements('[5,6]'::jsonb) WITH ORDINALITY e";
    let unaliased = r#"SELECT jsonb_each.key FROM jsonb_each('{"a":1}'::jsonb)"#;
    // Act
    let aliased_frames = query("json-elements-table-alias", &[], aliased, false);
    let unaliased_frames = query("json-elements-function-name", &[], unaliased, false);
    // Assert
    fixture::assert_success(&aliased_frames);
    fixture::assert_success(&unaliased_frames);
    assert_eq!(
        wire::data_rows(&aliased_frames),
        vec![row(&[Some("5"), Some("1")]), row(&[Some("6"), Some("2")])]
    );
    assert_eq!(wire::data_rows(&unaliased_frames), text_rows(&["a"]));
}

#[test]
fn should_correlate_with_preceding_table_given_lateral_keyword_and_implicit_lateral() {
    // Arrange
    let setup = docs_setup();
    let lateral = "SELECT t.id, e.value FROM json_elements_docs t, LATERAL jsonb_array_elements(t.doc) AS e ORDER BY t.id, e.value";
    let implicit = "SELECT t.id, e.value FROM json_elements_docs t, jsonb_array_elements(t.doc) e ORDER BY t.id, e.value";
    // Act
    let lateral_frames = query("json-elements-lateral", &setup, lateral, false);
    let implicit_frames = query("json-elements-implicit-lateral", &setup, implicit, false);
    // Assert
    let expected = vec![row(&[Some("1"), Some("1")]), row(&[Some("1"), Some("2")])];
    fixture::assert_success(&lateral_frames);
    fixture::assert_success(&implicit_frames);
    assert_eq!(wire::data_rows(&lateral_frames), expected);
    assert_eq!(wire::data_rows(&implicit_frames), expected);
}

#[test]
fn should_preserve_unmatched_left_rows_given_left_join_lateral_over_empty_and_null_documents() {
    // Arrange
    let setup = docs_setup();
    let sql = "SELECT t.id, e.value FROM json_elements_docs t LEFT JOIN LATERAL jsonb_array_elements(t.doc) e ON true ORDER BY t.id, e.value";
    // Act
    let frames = query("json-elements-left-lateral", &setup, sql, false);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("1"), Some("1")]),
            row(&[Some("1"), Some("2")]),
            row(&[Some("2"), None]),
            row(&[Some("3"), None]),
        ]
    );
}

#[test]
fn should_expand_each_row_given_lateral_each_over_a_jsonb_column() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_elements_objects (id INT, doc JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_elements_objects VALUES (1, '{\"b\":1,\"a\":2}'), (2, '{}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT t.id, e.key, e.value FROM json_elements_objects t, jsonb_each(t.doc) e ORDER BY t.id, e.key";
    // Act
    let frames = query("json-elements-lateral-each", &setup, sql, false);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("1"), Some("a"), Some("2")]),
            row(&[Some("1"), Some("b"), Some("1")]),
        ]
    );
}

#[test]
fn should_accept_a_cast_bind_parameter_given_jsonb_document_text() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT value FROM jsonb_array_elements($1::jsonb)",
        Some((0, 0, Some(b"[1, 2]"))),
        0,
        true,
    );
    // Act
    let frames = fixture::run_cycles("json-elements-cast-parameter", &[], vec![cycle]).remove(0);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![3802]);
    assert_eq!(wire::data_rows(&frames), text_rows(&["1", "2"]));
}

#[test]
fn should_infer_document_parameter_oids_given_unknown_parameters_in_json_and_jsonb_families() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle(
            "SELECT value FROM jsonb_array_elements($1)",
            Some((0, 0, Some(b"[1, 2]"))),
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT value FROM json_array_elements($1)",
            Some((0, 0, Some(b"[1, 2]"))),
            0,
            true,
        ),
    ];
    // Act
    let batches = fixture::run_cycles("json-elements-inferred-parameters", &[], cycles);
    // Assert
    for (frames, oid) in batches.iter().zip([3802, 114]) {
        fixture::assert_success(frames);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [oid]);
        assert_eq!(column_oids(frames), vec![oid]);
        assert_eq!(wire::data_rows(frames), text_rows(&["1", "2"]));
    }
}

#[test]
fn should_return_no_rows_given_a_null_bind_parameter() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT value FROM jsonb_array_elements($1::jsonb)",
        Some((0, 0, None)),
        0,
        false,
    );
    // Act
    let frames = fixture::run_cycles("json-elements-null-parameter", &[], vec![cycle]).remove(0);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
}
