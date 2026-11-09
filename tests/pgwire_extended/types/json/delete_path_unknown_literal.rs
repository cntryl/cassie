//! Deferred PostgreSQL 18.6 witnesses for unknown literal `#-` selectors.
use super::{fixture, wire};

#[test]
fn should_coerce_unknown_delete_paths_to_text_array_without_changing_other_overloads() {
    // Arrange
    let cycle = fixture::execute_cycle(
        r#"SELECT '{"a":{"b":1,"c":2}}'::JSONB #- '{a,b}', '{"a":1,"b":2}'::JSONB #- '{a}', '{"a":1}'::JSONB #- '{}', NULL::JSONB #- '{a}', '{"a":1}' #- '{a}'"#,
        None,
        0,
        true,
    );
    // Act
    let frames = fixture::run_cycles("jsonb-delete-unknown-path", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&frames[0]);
    assert!(
        frames[0].iter().any(|(tag, _)| *tag == b'1'),
        "ParseComplete"
    );
    let row_description = frames[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("row description");
    assert_eq!(
        wire::parse_row_description(&row_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802; 5]
    );
    assert_eq!(
        wire::data_rows(&frames[0]),
        vec![vec![
            Some("{\"a\": {\"c\": 2}}".into()),
            Some("{\"b\": 2}".into()),
            Some("{\"a\": 1}".into()),
            None,
            Some("{}".into())
        ]]
    );
}

#[test]
fn should_keep_explicit_text_and_bad_array_literal_diagnostics() {
    // Arrange
    let cases = [
        (r#"SELECT '{"a":1}'::JSONB #- '{a}'::TEXT"#, "42883"),
        (r#"SELECT '{"a":1}'::JSONB #- '{{a,b}}'"#, "2202E"),
        (r#"SELECT '{"a":1}'::JSONB #- '{a'"#, "22P02"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();
    // Act
    let frames = fixture::run_cycles("jsonb-delete-unknown-path-errors", &[], cycles);
    // Assert
    for ((_, expected), frames) in cases.iter().zip(frames) {
        assert_eq!(wire::error_code(&frames).as_deref(), Some(*expected));
    }
}

#[test]
fn should_preserve_delete_path_cast_error_priority_and_nulls() {
    // Arrange
    let cases = [
        ("SELECT 'bad'::JSONB #- '{a'", "json"),
        ("SELECT NULL::JSONB #- '{a'", "array"),
        ("SELECT 'bad'::JSONB #- NULL", "json"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();
    // Act
    let frames = fixture::run_cycles("jsonb-delete-unknown-path-priority", &[], cycles);
    // Assert
    for ((_, expected), frames) in cases.iter().zip(frames) {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22P02"));
        let fields = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .expect("error fields");
        assert!(
            fields
                .iter()
                .any(|(kind, message)| *kind == 'M'
                    && message.to_ascii_lowercase().contains(*expected)),
            "{fields:?}"
        );
    }
}

#[test]
fn should_keep_native_array_parameter_and_unbound_function_body_paths() {
    // Arrange
    let setup = [(
        r#"CREATE FUNCTION nested_delete_path() RETURNS TEXT AS "jsonb_build_object('a', jsonb_build_object('b', 7, 'c', 8)) #- '{a,b}'""#,
        Vec::new(),
    )];
    let cycles = vec![
        fixture::execute_cycle(
            r#"SELECT '{"a":{"b":7,"c":8}}'::JSONB #- $1"#,
            Some((1009, 0, Some(b"{a,b}"))),
            0,
            true,
        ),
        fixture::execute_cycle("SELECT nested_delete_path()", None, 0, true),
    ];
    // Act
    let frames = fixture::run_cycles("jsonb-delete-path-param-routine", &setup, cycles);
    // Assert
    for frames in frames {
        fixture::assert_success(&frames);
        assert_eq!(
            wire::data_rows(&frames),
            vec![vec![Some("{\"a\": {\"c\": 8}}".into())]]
        );
    }
}

#[test]
fn should_keep_existing_json_path_deletion_extension() {
    // Arrange
    let cycle = fixture::execute_cycle(r#"SELECT '{"a":1}'::JSON #- '{a}'"#, None, 0, true);
    // Act
    let frames = fixture::run_cycles("json-delete-unknown-path-extension", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&frames[0]);
    fixture::assert_single_column_descriptors(&frames[0], (114, -1, -1), 0, true);
    assert_eq!(wire::data_rows(&frames[0]), vec![vec![Some("{}".into())]]);
}
