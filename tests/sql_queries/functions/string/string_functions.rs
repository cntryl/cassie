//! PostgreSQL 18 string-function, formatting and indexing contracts for #803.

#[path = "residuals.rs"]
mod residual_semantics;
#[path = "resource_limits.rs"]
mod resource_limits;

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn failed_string_function_cases(fixture: &SqlFixture, cases: &[(&str, Value)]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![expected.clone()]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected:?}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect()
}

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
    let result = fixture.execute("SELECT split_part('a,b', ',', 3), split_part('a,b', ',', -3)");

    // Assert
    assert_eq!(
        result.expect("empty out-of-range fields").rows(),
        &[vec![
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
fn should_slice_unicode_text_for_signed_left_right_lengths() {
    // Arrange
    let fixture = sql_fixture("issue-803-left-right-negative-unicode", &[]);

    // Act
    let result = fixture.execute(
        "SELECT left('Aé🙂Z', -1), right('Aé🙂Z', -1), left('Aé🙂Z', 2), right('Aé🙂Z', 2)",
    );

    // Assert
    assert_eq!(
        result
            .expect("negative and positive Unicode slice lengths")
            .rows(),
        &[vec![
            Value::String("Aé🙂".to_owned()),
            Value::String("é🙂Z".to_owned()),
            Value::String("Aé".to_owned()),
            Value::String("🙂Z".to_owned()),
        ]]
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
    assert!(result.is_err(), "a NULL %I value is invalid: {result:?}");
    // The wire-facing SQLSTATE 22004 and PostgreSQL message are asserted in
    // `pgwire_extended::functions::string::error_sqlstates`.
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

#[test]
fn should_admit_a_bounded_repeat_output_expansion() {
    // Arrange
    let fixture = sql_fixture("issue-803-repeat-output-expansion", &[]);
    let expected = "ab".repeat(16 * 1024);

    // Act
    let result = fixture.execute("SELECT repeat('ab', 16384)");

    // Assert
    assert_eq!(
        result.expect("32 KiB repeated output").rows(),
        &[vec![Value::String(expected)]]
    );
}

#[test]
fn should_admit_bounded_multibyte_padding_output_expansion() {
    // Arrange
    let fixture = sql_fixture("issue-803-padding-output-expansion", &[]);

    // Act
    let result = fixture.execute(
        "SELECT char_length(lpad('x', 65536, 'ab')), left(lpad('x', 65536, 'ab'), 2), right(lpad('x', 65536, 'ab'), 2), char_length(rpad('x', 65536, 'ab')), left(rpad('x', 65536, 'ab'), 2), right(rpad('x', 65536, 'ab'), 2)",
    );

    // Assert
    assert_eq!(
        result.expect("bounded lpad/rpad expansion").rows(),
        &[vec![
            Value::Int64(65_536),
            Value::String("ab".to_owned()),
            Value::String("ax".to_owned()),
            Value::Int64(65_536),
            Value::String("xa".to_owned()),
            Value::String("ba".to_owned()),
        ]]
    );
}

#[test]
fn should_execute_postgresql_text_transformations_with_exact_results() {
    // Arrange
    let fixture = sql_fixture("issue-803-standard-text-transformations", &[]);
    let cases = [
        ("SELECT left('abcdef', 3)", Value::String("abc".to_owned())),
        ("SELECT right('abcdef', 2)", Value::String("ef".to_owned())),
        (
            "SELECT lpad('xy', 5, 'ab')",
            Value::String("abaxy".to_owned()),
        ),
        (
            "SELECT rpad('xy', 5, 'ab')",
            Value::String("xyaba".to_owned()),
        ),
        ("SELECT lpad('abcdef', 3)", Value::String("abc".to_owned())),
        ("SELECT rpad('abcdef', 3)", Value::String("abc".to_owned())),
        (
            "SELECT lpad('aé', 4, '🙂')",
            Value::String("🙂🙂aé".to_owned()),
        ),
        (
            "SELECT rpad('aé', 4, '🙂')",
            Value::String("aé🙂🙂".to_owned()),
        ),
        ("SELECT lpad('abc', -1)", Value::String(String::new())),
        ("SELECT rpad('abc', -1)", Value::String(String::new())),
        ("SELECT repeat('ab', 3)", Value::String("ababab".to_owned())),
        ("SELECT reverse('aé🙂')", Value::String("🙂éa".to_owned())),
        (
            "SELECT initcap('hELLO wORLD')",
            Value::String("Hello World".to_owned()),
        ),
        ("SELECT chr(9731)", Value::String("☃".to_owned())),
        ("SELECT ascii('é')", Value::Int64(233)),
        (
            "SELECT translate('abcdef', 'bd', 'XY')",
            Value::String("aXcYef".to_owned()),
        ),
        (
            "SELECT overlay('Txxxxas' PLACING 'hom' FROM 2 FOR 4)",
            Value::String("Thomas".to_owned()),
        ),
        (
            "SELECT overlay('Txxxxas' PLACING 'hom' FROM 2)",
            Value::String("Thomxas".to_owned()),
        ),
        (
            "SELECT concat_ws('-', 'a', NULL, 'c')",
            Value::String("a-c".to_owned()),
        ),
    ];

    // Act
    let failures = failed_string_function_cases(&fixture, &cases);

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL text transformations differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_distinguish_text_character_byte_length_from_string_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-803-text-length-and-predicates", &[]);
    let cases = [
        ("SELECT char_length('aé')", Value::Int64(2)),
        ("SELECT character_length('aé')", Value::Int64(2)),
        ("SELECT octet_length('aé')", Value::Int64(3)),
        ("SELECT bit_length('aé')", Value::Int64(24)),
        ("SELECT starts_with('alphabet', 'alpha')", Value::Bool(true)),
        ("SELECT 'alphabet' ^@ 'alpha'", Value::Bool(true)),
        ("SELECT 'alphabet' ^@ 'bet'", Value::Bool(false)),
        (
            "SELECT quote_literal('O''Brien')",
            Value::String("'O''Brien'".to_owned()),
        ),
        (
            "SELECT quote_nullable(CAST(NULL AS TEXT))",
            Value::String("NULL".to_owned()),
        ),
        ("SELECT quote_literal(42)", Value::String("'42'".to_owned())),
        ("SELECT to_hex(10)", Value::String("a".to_owned())),
        ("SELECT to_bin(10)", Value::String("1010".to_owned())),
        ("SELECT to_oct(10)", Value::String("12".to_owned())),
        ("SELECT to_hex(-1)", Value::String("ffffffff".to_owned())),
        (
            "SELECT to_bin(-1)",
            Value::String("11111111111111111111111111111111".to_owned()),
        ),
        ("SELECT to_oct(-1)", Value::String("37777777777".to_owned())),
        (
            "SELECT to_hex(-1::BIGINT)",
            Value::String("ffffffffffffffff".to_owned()),
        ),
        (
            "SELECT to_bin(-1::BIGINT)",
            Value::String(
                "1111111111111111111111111111111111111111111111111111111111111111".to_owned(),
            ),
        ),
        (
            "SELECT to_oct(-1::BIGINT)",
            Value::String("1777777777777777777777".to_owned()),
        ),
        (
            "SELECT num_nulls(1, CAST(NULL AS INT4), CAST(NULL AS TEXT))",
            Value::Int64(2),
        ),
        ("SELECT num_nonnulls(1, NULL, 'x')", Value::Int64(2)),
    ];

    // Act
    let failures = failed_string_function_cases(&fixture, &cases);

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL text length/predicate functions differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_match_text_prefix_with_starts_with_operator() {
    // Arrange
    let fixture = sql_fixture("issue-803-starts-with-operator", &[]);

    // Act
    let result =
        fixture.execute("SELECT 'alphabet' ^@ 'alpha', 'alphabet' ^@ 'beta', NULL::TEXT ^@ 'x'");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL text starts-with operator").rows(),
        &[vec![Value::Bool(true), Value::Bool(false), Value::Null]]
    );
}

#[test]
fn should_apply_postgresql_18_unicode_string_functions() {
    // Arrange
    let fixture = sql_fixture("issue-803-unicode-string-functions", &[]);
    let cases = [
        (
            "SELECT casefold('Straße' COLLATE \"und-x-icu\")",
            Value::String("strasse".to_owned()),
        ),
        ("SELECT unicode_assigned(U&'\\0441')", Value::Bool(true)),
        ("SELECT unicode_assigned(U&'\\+10FFFF')", Value::Bool(false)),
        (
            "SELECT unistr('\\0061\\+01F600')",
            Value::String("a😀".to_owned()),
        ),
        (
            "SELECT normalize(U&'e\\0301', NFC)",
            Value::String("é".to_owned()),
        ),
        (
            "SELECT normalize(U&'e\\0301')",
            Value::String("é".to_owned()),
        ),
        (
            "SELECT normalize(U&'\\00E9', NFD)",
            Value::String("e\u{301}".to_owned()),
        ),
        (
            "SELECT normalize(U&'\\00E9', NFKC)",
            Value::String("é".to_owned()),
        ),
        (
            "SELECT normalize(U&'\\00E9', NFKD)",
            Value::String("e\u{301}".to_owned()),
        ),
        ("SELECT U&'é' IS NFC NORMALIZED", Value::Bool(true)),
        ("SELECT U&'e\\0301' IS NFC NORMALIZED", Value::Bool(false)),
        ("SELECT U&'\\00E9' IS NFD NORMALIZED", Value::Bool(false)),
        ("SELECT U&'e\\0301' IS NFD NORMALIZED", Value::Bool(true)),
        ("SELECT U&'\\00E9' IS NFKC NORMALIZED", Value::Bool(true)),
        ("SELECT U&'\\00E9' IS NFKD NORMALIZED", Value::Bool(false)),
        (
            "SELECT to_ascii('abc', 'LATIN1')",
            Value::String("abc".to_owned()),
        ),
        ("SELECT to_ascii('abc', 8)", Value::String("abc".to_owned())),
    ];

    // Act
    let failures = failed_string_function_cases(&fixture, &cases);

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL 18 Unicode string functions differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_apply_negated_postgresql_unicode_normalization_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-803-negated-normalization-predicates", &[]);
    let cases = [
        ("SELECT 'e\u{301}' IS NOT NFC NORMALIZED", Value::Bool(true)),
        ("SELECT 'é' IS NOT NFD NORMALIZED", Value::Bool(true)),
        (
            "SELECT 'e\u{301}' IS NOT NFD NORMALIZED",
            Value::Bool(false),
        ),
        ("SELECT 'é' IS NOT NFC NORMALIZED", Value::Bool(false)),
        (
            "SELECT 'e\u{301}' IS NOT NFKC NORMALIZED",
            Value::Bool(true),
        ),
        ("SELECT 'é' IS NOT NFKC NORMALIZED", Value::Bool(false)),
        ("SELECT 'é' IS NOT NFKD NORMALIZED", Value::Bool(true)),
        (
            "SELECT 'e\u{301}' IS NOT NFKD NORMALIZED",
            Value::Bool(false),
        ),
        ("SELECT NULL IS NOT NFC NORMALIZED", Value::Null),
    ];

    // Act
    let failures = failed_string_function_cases(&fixture, &cases);

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL Unicode normalization predicates differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_preserve_empty_fields_for_two_argument_string_to_table() {
    // Arrange
    let fixture = sql_fixture("issue-803-string-to-table-two-arguments", &[]);

    // Act
    let rows = fixture
        .execute("SELECT string_to_table('a,,b', ',')")
        .expect("two-argument string_to_table");
    let null_input_rows = fixture
        .execute("SELECT string_to_table(CAST(NULL AS TEXT), ',')")
        .expect("NULL string_to_table input");

    // Assert
    assert_eq!(
        rows.rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String(String::new())],
            vec![Value::String("b".to_owned())],
        ]
    );
    assert_eq!(null_input_rows.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_execute_overlay_placing_keyword_form() {
    // Arrange
    let fixture = sql_fixture("issue-803-overlay-placing-keyword", &[]);

    // Act
    let result = fixture.execute("SELECT overlay('abcdef' PLACING 'XYZ' FROM 2 FOR 3)");

    // Assert
    assert_eq!(
        result.expect("OVERLAY PLACING FROM FOR form").rows(),
        &[vec![Value::String("aXYZef".to_owned())]]
    );
}

#[test]
fn should_propagate_null_through_postgresql18_string_function_signatures() {
    // Arrange
    let fixture = sql_fixture("issue-803-added-string-function-nulls", &[]);
    let cases = [
        (
            "SELECT concat_ws(CAST(NULL AS TEXT), 'a', 'b')",
            Value::Null,
        ),
        (
            "SELECT translate(CAST(NULL AS TEXT), 'a', 'b')",
            Value::Null,
        ),
        ("SELECT octet_length(CAST(NULL AS TEXT))", Value::Null),
        ("SELECT bit_length(CAST(NULL AS TEXT))", Value::Null),
        ("SELECT unicode_assigned(CAST(NULL AS TEXT))", Value::Null),
        ("SELECT unistr(CAST(NULL AS TEXT))", Value::Null),
        ("SELECT normalize(CAST(NULL AS TEXT))", Value::Null),
        ("SELECT to_ascii(CAST(NULL AS TEXT), 'LATIN1')", Value::Null),
        ("SELECT parse_ident(CAST(NULL AS TEXT))", Value::Null),
    ];

    // Act
    let failures = failed_string_function_cases(&fixture, &cases);

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL string function NULL behavior differs:\n{}",
        failures.join("\n")
    );
}
