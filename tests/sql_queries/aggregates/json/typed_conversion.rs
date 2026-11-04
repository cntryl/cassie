//! Typed collection values use the scalar JSON construction contract (#811).

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_exact_numeric_values_in_json_collection_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-811-exact-json-collection-numbers", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_agg(9007199254740993.125500::NUMERIC)::TEXT, \
                jsonb_agg(9007199254740993.125500::NUMERIC)::TEXT, \
                json_object_agg('n',9007199254740993.125500::NUMERIC)::TEXT, \
                jsonb_object_agg('n',9007199254740993.125500::NUMERIC)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("exact JSON aggregate numbers").rows(),
        &[vec![
            Value::String("[9007199254740993.125500]".into()),
            Value::String("[9007199254740993.125500]".into()),
            Value::String("{ \"n\" : 9007199254740993.125500 }".into()),
            Value::String("{\"n\": 9007199254740993.125500}".into()),
        ]]
    );
}

#[test]
fn should_preserve_typed_numeric_json_aggregate_spelling() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-real-and-special-values", &[]);
    let cases = [
        ("1.25::REAL", "1.25"),
        ("'NaN'::NUMERIC", "\"NaN\""),
        ("'Infinity'::NUMERIC", "\"Infinity\""),
        ("'-Infinity'::NUMERIC", "\"-Infinity\""),
        ("'NaN'::REAL", "\"NaN\""),
        ("'Infinity'::REAL", "\"Infinity\""),
        ("'-Infinity'::REAL", "\"-Infinity\""),
        ("'NaN'::FLOAT8", "\"NaN\""),
        ("'Infinity'::FLOAT8", "\"Infinity\""),
        ("'-Infinity'::FLOAT8", "\"-Infinity\""),
    ];

    for (input, expected) in cases {
        // Act
        let result = fixture.execute(&format!(
            "SELECT json_agg({input})::TEXT, jsonb_agg({input})::TEXT, \
                    json_object_agg('n',{input})::TEXT, jsonb_object_agg('n',{input})::TEXT"
        ));

        // Assert
        assert_eq!(
            result.expect("typed JSON aggregate conversion").rows(),
            &[vec![
                Value::String(format!("[{expected}]")),
                Value::String(format!("[{expected}]")),
                Value::String(format!("{{ \"n\" : {expected} }}")),
                Value::String(format!("{{\"n\": {expected}}}")),
            ]],
            "PostgreSQL 18.6 typed aggregate input: {input}"
        );
    }
}

#[test]
fn should_distinguish_json_documents_from_sql_text_in_collection_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-collection-document-identity", &[]);

    // Act
    let result = fixture.execute(
        r#"SELECT json_agg('{"a":1,"a":2}'::JSON)::TEXT,
                  jsonb_agg('{"a":1,"a":2}'::JSON)::TEXT,
                  jsonb_agg('{"a":1}'::TEXT)::TEXT,
                  jsonb_object_agg('doc',jsonb_build_object('a',1))::TEXT"#,
    );

    // Assert
    assert_eq!(
        result.expect("logical JSON input identity").rows(),
        &[vec![
            Value::String(r#"[{"a":1,"a":2}]"#.into()),
            Value::String(r#"[{"a": 2}]"#.into()),
            Value::String(r#"["{\"a\":1}"]"#.into()),
            Value::String(r#"{"doc": {"a": 1}}"#.into()),
        ]]
    );
}

#[test]
fn should_preserve_null_kind_collection_policies() {
    // Arrange
    let fixture = sql_fixture("issue-811-typed-json-collection-null-policies", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_agg(NULL::NUMERIC)::TEXT, jsonb_agg(NULL::REAL)::TEXT, \
                jsonb_agg('null'::JSONB)::TEXT, \
                jsonb_object_agg_strict('n','null'::JSONB)::TEXT, \
                json_object_agg_strict('n','null'::JSON)::TEXT, \
                jsonb_agg(NULL::NUMERIC) FILTER (WHERE FALSE)::TEXT, \
                json_agg_strict('null'::JSON)::TEXT, \
                jsonb_agg_strict('null'::JSONB)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("typed JSON aggregate NULL policies").rows(),
        &[vec![
            Value::String("[null]".into()),
            Value::String("[null]".into()),
            Value::String("[null]".into()),
            Value::String("{}".into()),
            Value::String("{ \"n\" : null }".into()),
            Value::Null,
            Value::String("[null]".into()),
            Value::String("[null]".into()),
        ]]
    );
}

#[test]
fn should_format_nonfinite_json_object_keys_as_scalar_sql_values() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-special-object-keys", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_object_agg('NaN'::NUMERIC,1)::TEXT, \
                jsonb_object_agg('Infinity'::FLOAT8,1)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("native scalar object-key text").rows(),
        &[vec![
            Value::String("{ \"NaN\" : 1 }".into()),
            Value::String("{\"Infinity\": 1}".into()),
        ]]
    );
}

#[test]
fn should_distinguish_filtered_empty_groups_from_strict_groups_with_null_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-strict-empty-distinction", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_agg_strict(NULL::INT)::TEXT, jsonb_agg_strict(NULL::INT)::TEXT, \
                json_object_agg_strict('n',NULL::INT)::TEXT, \
                jsonb_object_agg_strict('n',NULL::INT)::TEXT, \
                json_agg_strict(NULL::INT) FILTER (WHERE FALSE)::TEXT, \
                jsonb_object_agg_strict('n',NULL::INT) FILTER (WHERE FALSE)::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("strict aggregate group identity").rows(),
        &[vec![
            Value::String("[]".into()),
            Value::String("[]".into()),
            Value::String("{  }".into()),
            Value::String("{}".into()),
            Value::Null,
            Value::Null,
        ]]
    );
}

#[test]
fn should_preserve_json_real_negative_zero_while_canonicalizing_jsonb() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-real-negative-zero", &[]);

    // Act
    let result = fixture.execute("SELECT json_agg('-0'::REAL)::TEXT, jsonb_agg('-0'::REAL)::TEXT");

    // Assert
    assert_eq!(
        result.expect("native real JSON zero identity").rows(),
        &[vec![
            Value::String("[-0]".into()),
            Value::String("[0]".into())
        ]]
    );
}

#[test]
fn should_apply_numeric_aggregate_modifiers_before_json_conversion() {
    // Arrange
    let fixture = sql_fixture("issue-811-typed-json-collection-modifiers", &[]);

    // Act
    let result = fixture.execute(
        "SELECT jsonb_agg(DISTINCT n ORDER BY n) FILTER (WHERE selected)::TEXT \
         FROM (VALUES (2.50::NUMERIC,TRUE),(1.25::NUMERIC,TRUE), \
                      (2.50::NUMERIC,TRUE),(0::NUMERIC,FALSE)) AS inputs(n,selected)",
    );

    // Assert
    assert_eq!(
        result.expect("SQL modifiers precede JSON encoding").rows(),
        &[vec![Value::String("[1.25, 2.50]".into())]]
    );
}

#[test]
fn should_infer_json_conversion_identity_without_evaluating_a_filtered_first_row() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-conversion-filtered-leading-row", &[]);

    // Act
    let result = fixture.execute(
        "SELECT jsonb_agg(jsonb_build_object('n',10 / n)) FILTER (WHERE chosen)::TEXT \
         FROM (VALUES (0,FALSE),(2,TRUE)) AS inputs(n,chosen)",
    );

    // Assert
    assert_eq!(
        result
            .expect("bound metadata does not evaluate rejected input")
            .rows(),
        &[vec![Value::String("[{\"n\": 5}]".into())]]
    );
}

#[test]
fn should_use_the_statement_temporal_session_for_json_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-json-collection-session-time-zone",
        &["SET TIME ZONE 'America/New_York'"],
    );

    // Act
    let result = fixture.execute(
        "SELECT jsonb_agg(TIMESTAMPTZ '2024-11-03 06:30:00+00')::TEXT, \
                jsonb_object_agg('at',TIMESTAMPTZ '2024-11-03 05:30:00+00')::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("statement-local JSON timestamp values")
            .rows(),
        &[vec![
            Value::String("[\"2024-11-03T01:30:00-05:00\"]".into()),
            Value::String("{\"at\": \"2024-11-03T01:30:00-04:00\"}".into()),
        ]]
    );
}

#[test]
fn should_keep_json_calendar_spelling_independent_of_datestyle() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-json-construction-temporal-settings",
        &[
            "SET TIME ZONE 'America/New_York'",
            "SET DateStyle TO 'SQL, DMY'",
            "SET IntervalStyle TO 'postgres_verbose'",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT jsonb_agg(DATE '2024-11-03')::TEXT, \
                to_jsonb(DATE '2024-11-03')::TEXT, \
                jsonb_agg(TIMESTAMP '2024-11-03 01:30:00.125')::TEXT, \
                to_jsonb(TIMESTAMP '2024-11-03 01:30:00.125')::TEXT, \
                jsonb_agg(INTERVAL '1 year 2 days 03:04:05')::TEXT, \
                to_jsonb(INTERVAL '1 year 2 days 03:04:05')::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("SQL/JSON temporal output settings").rows(),
        &[vec![
            Value::String("[\"2024-11-03\"]".into()),
            Value::String("\"2024-11-03\"".into()),
            Value::String("[\"2024-11-03T01:30:00.125\"]".into()),
            Value::String("\"2024-11-03T01:30:00.125\"".into()),
            Value::String("[\"@ 1 year 2 days 3 hours 4 mins 5 secs\"]".into()),
            Value::String("\"@ 1 year 2 days 3 hours 4 mins 5 secs\"".into()),
        ]]
    );
}

#[test]
fn should_preserve_jsonb_document_identity_in_aggregate_operator_composition() {
    // Arrange
    let fixture = sql_fixture("jsonb-aggregate-document-composition", &[]);
    // Act
    let result = fixture.execute(
        "SELECT (jsonb_agg(n) || '[2]'::JSONB)::TEXT, \
        jsonb_build_object('items',jsonb_agg(n))::TEXT FROM (VALUES (1::INT)) AS inputs(n)",
    );
    // Assert
    assert_eq!(
        result.expect("aggregate is a JSONB document").rows(),
        &[vec![
            Value::String("[1, 2]".into()),
            Value::String("{\"items\": [1]}".into())
        ]]
    );
}

#[test]
fn should_preserve_json_timestamp_offset_policy_across_collection_conversion() {
    // Arrange
    let cases = [
        (
            "UTC",
            "TIMESTAMPTZ '2024-11-03 06:30:00+00'",
            "2024-11-03T06:30:00+00:00",
        ),
        (
            "Asia/Kolkata",
            "TIMESTAMPTZ '2024-11-03 06:30:00+00'",
            "2024-11-03T12:00:00+05:30",
        ),
        (
            "Europe/Amsterdam",
            "TIMESTAMPTZ '1900-01-01 00:00:00+00'",
            "1900-01-01T00:19:32+00:19:32",
        ),
        ("UTC", "TIMETZ '12:34:56+05'", "12:34:56+05"),
        (
            "UTC",
            "TIMESTAMPTZ '0001-01-01 00:00:00+00 BC'",
            "0001-01-01T00:00:00+00:00 BC",
        ),
        ("UTC", "TIMESTAMPTZ 'infinity'", "infinity"),
        ("UTC", "TIMESTAMPTZ '-infinity'", "-infinity"),
    ];
    for (zone, input, expected) in cases {
        let fixture = sql_fixture(
            "jsonb-aggregate-timestamp-offset",
            &[&format!("SET TIME ZONE '{zone}'")],
        );
        // Act
        let result = fixture.execute(&format!(
            "SELECT to_jsonb({input})::TEXT,jsonb_agg({input})::TEXT"
        ));
        // Assert
        let document = serde_json::to_string(expected).expect("JSON string");
        assert_eq!(
            result.expect("JSON temporal correspondence").rows(),
            &[vec![
                Value::String(document.clone()),
                Value::String(format!("[{document}]"))
            ]],
            "{zone}: {input}"
        );
    }
}
