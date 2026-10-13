//! PostgreSQL 18 string-function, formatting and indexing contracts for #803.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_split_part_at_a_multicharacter_delimiter() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-multicharacter", &[]);

    // Act
    let result = fixture.execute("SELECT split_part('abc~@~def~@~ghi', '~@~', 2)");

    // Assert
    assert_eq!(
        result.expect("second split field").rows(),
        &[vec![Value::String("def".to_owned())]]
    );
}

#[test]
fn should_select_negative_split_part_indexes_from_the_end() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-negative", &[]);

    // Act
    let result = fixture.execute("SELECT split_part('abc,def,ghi,jkl', ',', -2)");

    // Assert
    assert_eq!(
        result.expect("second split field from end").rows(),
        &[vec![Value::String("ghi".to_owned())]]
    );
}

#[test]
fn should_return_empty_split_part_for_invalid_indexes() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-out-of-range", &[]);

    // Act
    let result = fixture.execute(
        "SELECT split_part('a,b', ',', 0), split_part('a,b', ',', 3), split_part('a,b', ',', -3)",
    );

    // Assert
    assert_eq!(
        result.expect("empty out-of-range fields").rows(),
        &[vec![
            Value::String(String::new()),
            Value::String(String::new()),
            Value::String(String::new()),
        ]]
    );
}

#[test]
fn should_trim_character_sets_from_both_ends_with_btrim() {
    // Arrange
    let fixture = sql_fixture("issue-803-btrim-character-set", &[]);

    // Act
    let result = fixture.execute("SELECT btrim('xyxtrimyyx', 'xyz')");

    // Assert
    assert_eq!(
        result.expect("btrim character set").rows(),
        &[vec![Value::String("trim".to_owned())]]
    );
}

#[test]
fn should_trim_character_sets_from_the_left_with_ltrim() {
    // Arrange
    let fixture = sql_fixture("issue-803-ltrim-character-set", &[]);

    // Act
    let result = fixture.execute("SELECT ltrim('zzzytest', 'xyz')");

    // Assert
    assert_eq!(
        result.expect("ltrim character set").rows(),
        &[vec![Value::String("test".to_owned())]]
    );
}

#[test]
fn should_trim_character_sets_from_the_right_with_rtrim() {
    // Arrange
    let fixture = sql_fixture("issue-803-rtrim-character-set", &[]);

    // Act
    let result = fixture.execute("SELECT rtrim('testxxzx', 'xyz')");

    // Assert
    assert_eq!(
        result.expect("rtrim character set").rows(),
        &[vec![Value::String("test".to_owned())]]
    );
}

#[test]
fn should_support_substr_as_a_substring_alias() {
    // Arrange
    let fixture = sql_fixture("issue-803-substr-alias", &[]);

    // Act
    let result = fixture.execute("SELECT substr('alphabet', 3), substr('alphabet', 3, 2)");

    // Assert
    assert_eq!(
        result.expect("substr alias").rows(),
        &[vec![
            Value::String("phabet".to_owned()),
            Value::String("ph".to_owned()),
        ]]
    );
}

#[test]
fn should_index_substr_by_characters_for_utf8_text() {
    // Arrange
    let fixture = sql_fixture("issue-803-substr-character-indexing", &[]);

    // Act
    let result = fixture.execute("SELECT substr('Aé🙂Z', 2, 2)");

    // Assert
    assert_eq!(
        result.expect("UTF-8 character substring").rows(),
        &[vec![Value::String("é🙂".to_owned())]]
    );
}

#[test]
fn should_count_utf8_characters_for_strpos() {
    // Arrange
    let fixture = sql_fixture("issue-803-strpos-character-indexing", &[]);

    // Act
    let result = fixture.execute("SELECT strpos('Aé🙂Z', '🙂')");

    // Assert
    assert_eq!(
        result.expect("UTF-8 character position").rows(),
        &[vec![Value::Int64(3)]]
    );
}

#[test]
fn should_replace_all_occurrences_of_a_substring() {
    // Arrange
    let fixture = sql_fixture("issue-803-replace-all", &[]);

    // Act
    let result = fixture.execute("SELECT replace('abcdefabcdef', 'cd', 'XX')");

    // Assert
    assert_eq!(
        result.expect("replace all occurrences").rows(),
        &[vec![Value::String("abXXefabXXef".to_owned())]]
    );
}

#[test]
fn should_return_zero_for_a_missing_substring_with_strpos() {
    // Arrange
    let fixture = sql_fixture("issue-803-strpos", &[]);

    // Act
    let result = fixture.execute("SELECT strpos('high', 'ig'), strpos('high', 'missing')");

    // Assert
    assert_eq!(
        result.expect("strpos positions").rows(),
        &[vec![Value::Int64(2), Value::Int64(0)]]
    );
}

#[test]
fn should_execute_position_keyword_form_with_one_based_indexing() {
    // Arrange
    let fixture = sql_fixture("issue-803-position-keyword", &[]);

    // Act
    let result = fixture.execute("SELECT position('om' IN 'Thomas')");

    // Assert
    assert_eq!(
        result.expect("POSITION substring IN string").rows(),
        &[vec![Value::Int64(3)]]
    );
}

#[test]
fn should_extract_substring_from_for_keyword_forms() {
    // Arrange
    let fixture = sql_fixture("issue-803-substring-keyword", &[]);

    // Act
    let result = fixture.execute(
        "SELECT SUBSTRING('Thomas' FROM 2 FOR 3), SUBSTRING('Thomas' FROM 3), SUBSTRING('Thomas' FOR 2)",
    );

    // Assert
    assert_eq!(
        result.expect("SUBSTRING keyword forms").rows(),
        &[vec![
            Value::String("hom".to_owned()),
            Value::String("omas".to_owned()),
            Value::String("Th".to_owned()),
        ]]
    );
}

#[test]
fn should_trim_only_the_selected_ends_with_keyword_forms() {
    // Arrange
    let fixture = sql_fixture("issue-803-trim-keyword-directions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT TRIM(LEADING 'xy' FROM 'xyTitle'), TRIM(TRAILING 'xy' FROM 'Titlexy'), TRIM(BOTH 'xy' FROM 'xyTitlexy')",
    );

    // Assert
    assert_eq!(
        result.expect("TRIM direction forms").rows(),
        &[vec![
            Value::String("Title".to_owned()),
            Value::String("Title".to_owned()),
            Value::String("Title".to_owned()),
        ]]
    );
}

#[test]
fn should_default_trim_keyword_form_to_spaces() {
    // Arrange
    let fixture = sql_fixture("issue-803-trim-default-space", &[]);

    // Act
    let result =
        fixture.execute("SELECT TRIM(LEADING FROM '  title  '), TRIM(TRAILING FROM '  title  ')");

    // Assert
    assert_eq!(
        result.expect("TRIM default space set").rows(),
        &[vec![
            Value::String("title  ".to_owned()),
            Value::String("  title".to_owned()),
        ]]
    );
}

#[test]
fn should_format_all_supported_argument_kinds() {
    // Arrange
    let fixture = sql_fixture("issue-803-format-conversions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT format('%I|%L|%s', 'Foo bar', 'O''Reilly', 42), format('%2$s %1$s', 'first', 'second')",
    );

    // Assert
    assert_eq!(
        result.expect("format conversion types").rows(),
        &[vec![
            Value::String("\"Foo bar\"|'O''Reilly'|42".to_owned()),
            Value::String("second first".to_owned()),
        ]]
    );
}

#[test]
fn should_format_null_as_empty_string_or_unquoted_literal_by_specifier() {
    // Arrange
    let fixture = sql_fixture("issue-803-format-null", &[]);

    // Act
    let result = fixture.execute("SELECT format('%s|%L|%%', NULL, NULL)");

    // Assert
    assert_eq!(
        result.expect("format NULL and literal percent").rows(),
        &[vec![Value::String("|NULL|%".to_owned())]]
    );
}

#[test]
fn should_reject_null_identifier_format_arguments() {
    // Arrange
    let fixture = sql_fixture("issue-803-format-null-identifier", &[]);

    // Act
    let result = fixture.execute("SELECT format('%I', CAST(NULL AS TEXT))");

    // Assert
    assert!(
        matches!(result, Err(CassieError::InvalidParameterValue(_))),
        "a NULL %I value is invalid: {result:?}"
    );
}

#[test]
fn should_propagate_null_arguments_through_string_helpers() {
    // Arrange
    let fixture = sql_fixture("issue-803-string-helper-nulls", &[]);

    // Act
    let result = fixture.execute(
        "SELECT split_part(NULL, ',', 1), btrim(NULL), replace(NULL, 'x', 'y'), strpos(NULL, 'x'), substr(NULL, 1)",
    );

    // Assert
    assert_eq!(
        result.expect("NULL string inputs").rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null
        ]]
    );
}

#[test]
fn should_propagate_null_for_position_keyword_form() {
    // Arrange
    let fixture = sql_fixture("issue-803-position-null", &[]);

    // Act
    let result = fixture.execute("SELECT position(NULL IN 'abc')");

    // Assert
    assert_eq!(
        result.expect("NULL POSITION argument").rows(),
        &[vec![Value::Null]]
    );
}

#[test]
fn should_bind_parameters_to_split_part_signature() {
    // Arrange
    let fixture = sql_fixture("issue-803-split-part-parameters", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT split_part($1, $2, $3)",
        vec![
            Value::String("one,two,three".to_owned()),
            Value::String(",".to_owned()),
            Value::Int64(2),
        ],
    );

    // Assert
    assert_eq!(
        result.expect("split_part bound arguments").rows(),
        &[vec![Value::String("two".to_owned())]]
    );
}

#[test]
fn should_admit_a_bounded_format_output_expansion() {
    // Arrange
    let fixture = sql_fixture("issue-803-format-output-expansion", &[]);
    let part = "x".repeat(32 * 1024);
    let expected = part.repeat(2);
    let sql = format!("SELECT format('%s%s', '{part}', '{part}')");

    // Act
    let result = fixture.execute(&sql);

    // Assert
    assert_eq!(
        result.expect("64 KiB formatted output").rows(),
        &[vec![Value::String(expected)]]
    );
}
