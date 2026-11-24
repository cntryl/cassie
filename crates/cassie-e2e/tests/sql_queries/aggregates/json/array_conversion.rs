//! Native SQL arrays use the scalar SQL-to-JSON element contract.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_preserve_exact_numeric_array_elements_in_json_construction() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-native-numeric-array", &[]);
    let input =
        "ARRAY[[9007199254740993.125500::NUMERIC,'NaN'::NUMERIC],[NULL,'Infinity'::NUMERIC]]";

    // Act
    let result = fixture.execute(&format!(
        "SELECT to_json({input})::TEXT,to_jsonb({input})::TEXT,json_agg({input})::TEXT,jsonb_agg({input})::TEXT"
    ));

    // Assert
    assert_eq!(
        result.expect("exact native numeric arrays").rows(),
        &[vec![
            Value::String(r#"[[9007199254740993.125500,"NaN"],[null,"Infinity"]]"#.into()),
            Value::String(r#"[[9007199254740993.125500, "NaN"], [null, "Infinity"]]"#.into()),
            Value::String(r#"[[[9007199254740993.125500,"NaN"],[null,"Infinity"]]]"#.into()),
            Value::String(r#"[[[9007199254740993.125500, "NaN"], [null, "Infinity"]]]"#.into()),
        ]]
    );
}

#[test]
fn should_format_nonfinite_native_array_numbers_as_json_strings() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-native-nonfinite-array", &[]);
    for kind in ["REAL", "FLOAT8"] {
        let input =
            format!("ARRAY[1.25::{kind},'NaN'::{kind},'Infinity'::{kind},'-Infinity'::{kind}]");

        // Act
        let result = fixture.execute(&format!(
            "SELECT to_json({input})::TEXT,jsonb_object_agg('v',{input})::TEXT"
        ));

        // Assert
        assert_eq!(
            result
                .expect("finite and nonfinite array conversion")
                .rows(),
            &[vec![
                Value::String(r#"[1.25,"NaN","Infinity","-Infinity"]"#.into()),
                Value::String(r#"{"v": [1.25, "NaN", "Infinity", "-Infinity"]}"#.into()),
            ]],
            "array element family: {kind}"
        );
    }
}

#[test]
fn should_preserve_document_identity_inside_native_json_arrays() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-native-document-array", &[]);

    // Act
    let result = fixture.execute(
        r#"SELECT
        to_json(ARRAY['{"a":1,"a":2}'::JSON,NULL,'null'::JSON])::TEXT,
        to_jsonb(ARRAY['{"a":1,"a":2}'::JSON,NULL,'null'::JSON])::TEXT,
        to_json(ARRAY['{"a":1}'::TEXT,'null',NULL])::TEXT"#,
    );

    // Assert
    assert_eq!(
        result.expect("array element document identity").rows(),
        &[vec![
            Value::String(r#"[{"a":1,"a":2},null,null]"#.into()),
            Value::String(r#"[{"a": 2}, null, null]"#.into()),
            Value::String(r#"["{\"a\":1}","null",null]"#.into()),
        ]]
    );
}

#[test]
fn should_preserve_native_array_shape_when_rendering_json() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-native-array-shape", &[]);

    // Act
    let result = fixture.execute(
        "SELECT to_json('[-2:-1][4:5]={{1,NULL},{3,4}}'::INT[])::TEXT, \
        array_lower('[-2:-1][4:5]={{1,NULL},{3,4}}'::INT[],1), \
        array_lower('[-2:-1][4:5]={{1,NULL},{3,4}}'::INT[],2), \
        to_json('{}'::NUMERIC[])::TEXT,to_json(NULL::NUMERIC[]) IS NULL, \
        to_json(ARRAY[[[[[[1::NUMERIC]]]]]])::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("shape without bound mutation").rows(),
        &[vec![
            Value::String("[[1,null],[3,4]]".into()),
            Value::Int64(-2),
            Value::Int64(4),
            Value::String("[]".into()),
            Value::Bool(true),
            Value::String("[[[[[[1]]]]]]".into()),
        ]]
    );
}

#[test]
fn should_use_statement_temporal_settings_for_native_json_array_elements() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-json-native-temporal-array",
        &[
            "SET TIME ZONE 'America/New_York'",
            "SET DateStyle TO 'SQL, DMY'",
            "SET IntervalStyle TO 'sql_standard'",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT \
        jsonb_agg(ARRAY[TIMESTAMPTZ '2024-11-03 06:30:00+00','infinity'::TIMESTAMPTZ])::TEXT, \
        to_json(ARRAY[DATE '2024-01-02'])::TEXT, \
        to_json(ARRAY[TIMESTAMP '2024-01-02 03:04:05'])::TEXT, \
        to_json(ARRAY[TIMETZ '12:34:56+05'])::TEXT, \
        to_json(ARRAY[INTERVAL '1 day 2 hours'])::TEXT",
    );

    // Assert
    assert_eq!(
        result
            .expect("statement-local array element conversion")
            .rows(),
        &[vec![
            Value::String(r#"[["2024-11-03T01:30:00-05:00", "infinity"]]"#.into()),
            Value::String(r#"["2024-01-02"]"#.into()),
            Value::String(r#"["2024-01-02T03:04:05"]"#.into()),
            Value::String(r#"["12:34:56+05"]"#.into()),
            Value::String(r#"["1 2:00:00"]"#.into()),
        ]]
    );
}

#[test]
fn should_use_logical_padding_for_bounded_stored_character_json_arrays() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-json-stored-character-array",
        &[
            "CREATE TABLE json_character_arrays (id INT PRIMARY KEY,items CHAR(3)[])",
            "INSERT INTO json_character_arrays VALUES (1,ARRAY['x'::CHAR(3),NULL])",
        ],
    );

    // Act
    let scalar = fixture
        .execute("SELECT to_json(items)::TEXT,to_jsonb(items)::TEXT FROM json_character_arrays");
    let aggregate = fixture.execute(
        "SELECT json_agg(items)::TEXT,jsonb_object_agg('v',items)::TEXT FROM json_character_arrays",
    );

    // Assert
    assert_eq!(
        scalar
            .expect("logical stored array scalar correspondence")
            .rows(),
        &[vec![
            Value::String(r#"["x  ",null]"#.into()),
            Value::String(r#"["x  ", null]"#.into()),
        ]]
    );
    assert_eq!(
        aggregate
            .expect("logical stored array aggregate correspondence")
            .rows(),
        &[vec![
            Value::String(r#"[["x  ",null]]"#.into()),
            Value::String(r#"{"v": ["x  ", null]}"#.into()),
        ]]
    );
}
