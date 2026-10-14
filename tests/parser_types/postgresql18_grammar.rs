//! PostgreSQL 18 grammar and expression contracts tracked by #796.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

type ValuePredicate = fn(&Value) -> bool;

#[test]
fn should_bind_schema_qualified_postgres_type_names() {
    // Arrange
    let fixture = sql_fixture("issue-796-qualified-type-name", &[]);

    // Act
    let result = fixture.execute("SELECT '7'::pg_catalog.int4 AS value");

    // Assert
    assert_eq!(
        result.expect("schema-qualified PostgreSQL type").rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_bind_double_precision_type_names_in_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-double-precision", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('1.5' AS DOUBLE PRECISION) AS value");

    // Assert
    assert_eq!(
        result.expect("DOUBLE PRECISION cast").rows(),
        &[vec![Value::Float64(1.5)]]
    );
}

#[test]
fn should_map_float_precision_boundaries_to_postgres_type_oids() {
    // Arrange
    let label = "issue-796-float-precision-boundaries";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('1.5' AS FLOAT(24)) AS value", 700),
        ("SELECT CAST('1.5' AS FLOAT(25)) AS value", 701),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::Float64(1.5)]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value 1.5, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "float precision cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_accept_postgres_float_type_aliases() {
    // Arrange
    let label = "issue-796-float-type-aliases";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('1.25' AS float4) AS value", 700),
        ("SELECT CAST('1.25' AS float8) AS value", 701),
        ("SELECT CAST('1.25' AS real) AS value", 700),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::Float64(1.25)]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value 1.25, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "float alias cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_bind_postgres_character_type_forms() {
    // Arrange
    let label = "issue-796-character-type-aliases";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('word' AS character varying(4)) AS value", 1043),
        ("SELECT CAST('word' AS char varying(4)) AS value", 1043),
        (
            "SELECT CAST('word' AS national character varying(4)) AS value",
            1043,
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::String("word".to_owned())]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value word, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "character type cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_bind_postgres_bpchar_name_type_spellings() {
    // Arrange
    let label = "issue-796-type-synonyms";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT CAST('x' AS bpchar) AS value", 1042),
        ("SELECT CAST('x' AS name) AS value", 19),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result)
                if result.rows() == [vec![Value::String("x".to_owned())]]
                    && result.columns()[0].type_oid == *expected_oid =>
            {
                None
            }
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid} and value x, got {:?} / {}",
                result.rows(),
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "character type aliases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_bind_postgres_type_spellings_with_native_type_oids() {
    // Arrange
    let fixture = sql_fixture("issue-796-type-name-oids", &[]);
    let cases = [
        ("SELECT NULL::character(2)", 1042),
        ("SELECT NULL::bit varying", 1562),
        ("SELECT NULL::time with time zone", 1266),
        ("SELECT NULL::time without time zone", 1083),
        ("SELECT NULL::timestamp with time zone", 1184),
        ("SELECT NULL::timestamp without time zone", 1114),
        ("SELECT NULL::national character varying(4)", 1043),
        ("SELECT NULL::nchar(4)", 1042),
        ("SELECT NULL::char varying(4)", 1043),
        ("SELECT NULL::\"char\"", 18),
        ("SELECT NULL::integer[]", 34_023),
        ("SELECT NULL::float4", 700),
        ("SELECT NULL::float8", 701),
        ("SELECT NULL::real", 700),
        ("SELECT NULL::float(24)", 700),
        ("SELECT NULL::float(25)", 701),
        ("SELECT NULL::dec", 1700),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_oid)| match fixture.execute(sql) {
            Ok(result) if result.columns()[0].type_oid == *expected_oid => None,
            Ok(result) => Some(format!(
                "{sql}: expected oid {expected_oid}, got {}",
                result.columns()[0].type_oid
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "PostgreSQL type-name cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_evaluate_postgres_function_style_type_casts() {
    // Arrange
    let label = "issue-796-function-style-casts";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT int4('7') AS value", vec![vec![Value::Int64(7)]], 23),
        (
            "SELECT float8('1.5') AS value",
            vec![vec![Value::Float64(1.5)]],
            701,
        ),
        (
            "SELECT text(7) AS value",
            vec![vec![Value::String("7".to_owned())]],
            25,
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(
            |(sql, expected_rows, expected_oid)| match fixture.execute(sql) {
                Ok(result)
                    if result.rows() == expected_rows
                        && result.columns()[0].type_oid == *expected_oid =>
                {
                    None
                }
                Ok(result) => Some(format!(
                    "{sql}: expected {expected_rows:?} with oid {expected_oid}, got {:?} / {}",
                    result.rows(),
                    result.columns()[0].type_oid
                )),
                Err(error) => Some(format!("{sql}: {error}")),
            },
        )
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "function-style casts failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_evaluate_a_function_style_date_cast() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-function-style-date-cast",
        &[
            "CREATE TABLE issue_796_date_cast (value TIMESTAMP)",
            "INSERT INTO issue_796_date_cast VALUES ('2001-02-16 20:38:40')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT DATE(value) AS value FROM issue_796_date_cast");

    // Assert
    let result = result.expect("DATE(timestamp) binds as a type cast");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("2001-02-16".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 1082);
}

#[test]
fn should_parse_escape_string_constants_with_postgres_backslash_rules() {
    // Arrange
    let fixture = sql_fixture("issue-796-escape-string", &[]);

    // Act
    let result = fixture.execute(r"SELECT E'line\nfeed' AS value");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL escape string").rows(),
        &[vec![Value::String("line\nfeed".to_owned())]]
    );
}

#[test]
fn should_parse_unicode_escape_string_constant_forms() {
    // Arrange
    let fixture = sql_fixture("issue-796-unicode-escape-string", &[]);

    // Act
    let result = fixture.execute("SELECT U&'d!0061t' UESCAPE '!' AS value");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL Unicode escape string").rows(),
        &[vec![Value::String("dat".to_owned())]]
    );
}

#[test]
fn should_parse_dollar_quoted_constants_in_expression_positions() {
    // Arrange
    let label = "issue-796-dollar-quoted-constant";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        (
            "SELECT $$alpha ' beta$$ AS value",
            vec![vec![Value::String("alpha ' beta".to_owned())]],
        ),
        (
            "SELECT coalesce($tag_2$gamma$tag_2$, 'fallback') AS value",
            vec![vec![Value::String("gamma".to_owned())]],
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected_rows)| match fixture.execute(sql) {
            Ok(result) if result.rows() == expected_rows => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected_rows:?}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "dollar-quoted literal cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_parse_postgres_integer_literal_forms() {
    // Arrange
    let label = "issue-796-radix-and-separated-integers";
    let fixture = sql_fixture(label, &[]);

    // Act
    let cases = [
        ("SELECT 0x2a AS value", 42),
        ("SELECT 0o52 AS value", 42),
        ("SELECT 0b101010 AS value", 42),
        ("SELECT 1_000_000 AS value", 1_000_000),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![Value::Int64(*expected)]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected integer {expected}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "integer literal cases failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_apply_postgres_modulo_bitwise_or_operators() {
    // Arrange
    let label = "issue-796-modulo-and-bitwise-or";

    // Act
    let result = sql_fixture(label, &[]).execute("SELECT 5 % 2, 5 | 2");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL modulo and bitwise OR").rows(),
        &[vec![Value::Int64(1), Value::Int64(7)]]
    );
}

#[test]
fn should_join_adjacent_string_constants_across_newlines() {
    // Arrange
    let fixture = sql_fixture("issue-796-adjacent-string-constants", &[]);

    // Act
    let result = fixture.execute("SELECT 'alpha'\n       'beta' AS value");

    // Assert
    assert_eq!(
        result.expect("newline-separated string literals").rows(),
        &[vec![Value::String("alphabeta".to_owned())]]
    );
}

#[test]
fn should_resolve_unicode_escaped_identifiers() {
    // Arrange
    let fixture = sql_fixture("issue-796-unicode-escaped-identifier", &[]);

    // Act
    let created = fixture.execute("CREATE TABLE unicode_names (U&\"na!006de\" UESCAPE '!' TEXT)");
    let inserted = fixture.execute("INSERT INTO unicode_names (name) VALUES ('resolved')");
    let selected = fixture.execute("SELECT name FROM unicode_names");

    // Assert
    created.expect("Unicode escaped identifier DDL");
    inserted.expect("Unicode escaped identifier binding");
    assert_eq!(
        selected.expect("Unicode escaped identifier lookup").rows(),
        &[vec![Value::String("resolved".to_owned())]]
    );
}

#[test]
fn should_apply_custom_unicode_escape_to_quoted_identifiers() {
    // Arrange
    let fixture = sql_fixture("issue-796-custom-unicode-identifier-escape", &[]);

    // Act
    let created = fixture.execute("CREATE TABLE U&\"issue_796_!0065scape\" UESCAPE '!' (id INT)");
    let inserted = fixture.execute("INSERT INTO issue_796_escape VALUES (42)");
    let selected =
        fixture.execute("SELECT id FROM U&\"issue_796_!0065scape\" UESCAPE '!' ORDER BY id");

    // Assert
    created.expect("create a table using a custom Unicode identifier escape");
    inserted.expect("insert through the decoded identifier spelling");
    assert_eq!(
        selected
            .expect("resolve the custom-escaped identifier on a later query")
            .rows(),
        &[vec![Value::Int64(42)]]
    );
}

#[test]
fn should_truncate_utf8_quoted_identifiers_at_postgres_byte_limit() {
    // Arrange
    let fixture = sql_fixture("issue-796-utf8-identifier-truncation", &[]);
    let retained = format!("{}éx", "a".repeat(60));
    let oversized = format!("{}éxyz", "a".repeat(60));

    // Act
    let result = fixture.execute(&format!(
        "SELECT \"{retained}\" FROM (SELECT 7 AS \"{oversized}\") AS names"
    ));

    // Assert
    assert_eq!(
        result
            .expect("PostgreSQL truncates identifiers by UTF-8 byte length")
            .rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_evaluate_symmetric_between_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-between-symmetric", &[]);

    // Act
    let result =
        fixture.execute("SELECT 2 BETWEEN SYMMETRIC 3 AND 1, 4 NOT BETWEEN SYMMETRIC 3 AND 1");

    // Assert
    assert_eq!(
        result.expect("symmetric BETWEEN predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_is_truth_value_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-truth-predicates", &[]);

    // Act
    let result = fixture
        .execute("SELECT TRUE IS TRUE, FALSE IS NOT TRUE, NULL IS UNKNOWN, TRUE IS NOT UNKNOWN");

    // Assert
    assert_eq!(
        result.expect("truth-value predicates").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_accept_postgres_postfix_null_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-postfix-null-predicates", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ISNULL, 1 NOTNULL");

    // Assert
    assert_eq!(
        result.expect("postfix null predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_concatenate_text_with_non_text_scalar_values() {
    // Arrange
    let fixture = sql_fixture("issue-796-polymorphic-text-concatenation", &[]);

    // Act
    let result = fixture.execute("SELECT 'Value: ' || 42 AS value");

    // Assert
    assert_eq!(
        result.expect("text concatenation with integer").rows(),
        &[vec![Value::String("Value: 42".to_owned())]]
    );
}

#[test]
fn should_concatenate_a_numeric_expression_before_text() {
    // Arrange
    let fixture = sql_fixture("issue-796-non-text-before-text-concatenation", &[]);

    // Act
    let result = fixture.execute("SELECT (1 + 2) || 'b' AS value");

    // Assert
    let result = result.expect("a non-array scalar may precede a text operand");
    assert_eq!(result.rows(), &[vec![Value::String("3b".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_resolve_schema_qualified_binary_operators() {
    // Arrange
    let label = "issue-796-qualified-operators";
    let fixture = sql_fixture(label, &[]);

    // Act
    let result = fixture.execute("SELECT 1 OPERATOR(pg_catalog.+) 2 AS value");

    // Assert
    assert_eq!(
        result.expect("qualified binary operator").rows(),
        &[vec![Value::Int64(3)]]
    );
}

#[test]
fn should_resolve_schema_qualified_unary_operators() {
    // Arrange
    let fixture = sql_fixture("issue-796-qualified-unary-operator", &[]);

    // Act
    let result = fixture.execute("SELECT OPERATOR(pg_catalog.-) 2 AS value");

    // Assert
    assert_eq!(
        result.expect("qualified unary operator").rows(),
        &[vec![Value::Int64(-2)]]
    );
}

#[test]
fn should_use_left_associative_postgres_power_with_float8_result() {
    // Arrange
    let fixture = sql_fixture("issue-796-bitwise-xor-precedence", &[]);

    // Act
    let result = fixture.execute("SELECT 2 ^ 3 ^ 2 AS value");

    // Assert
    let result = result.expect("left-associative floating-point exponentiation");
    assert_eq!(result.rows(), &[vec![Value::Float64(64.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_bind_postgres_chained_type_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-chained-type-casts", &[]);

    // Act
    let result = fixture.execute("SELECT '7'::TEXT::INT4 AS chained");

    // Assert
    let result = result.expect("chained casts bind through the preceding target type");
    assert_eq!(result.rows(), &[vec![Value::Int64(7)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_index_a_postgres_array_after_casting_it() {
    // Arrange
    let fixture = sql_fixture("issue-796-array-suffix-after-cast", &[]);

    // Act
    let result = fixture.execute("SELECT ('{10,20}'::INT[])[2] AS second_element");

    // Assert
    let result = result.expect("array subscript follows the cast expression");
    assert_eq!(result.rows(), &[vec![Value::Int64(20)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_apply_postgres_unary_minus_precedence_around_power() {
    // Arrange
    let fixture = sql_fixture("issue-796-unary-power-precedence", &[]);

    // Act
    let result =
        fixture.execute("SELECT -2 ^ 2 AS unary_before_power, -(2::INT4 ^ 2) AS grouped_power");

    // Assert
    let result = result.expect("unary minus and parentheses follow PostgreSQL power precedence");
    assert_eq!(
        result.rows(),
        &[vec![Value::Float64(4.0), Value::Float64(-4.0)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );
}

#[test]
fn should_accept_a_postgres_type_name_string_literal() {
    // Arrange
    let fixture = sql_fixture("issue-796-generic-type-string-literal", &[]);

    // Act
    let result = fixture.execute("SELECT INTEGER '42' AS value");

    // Assert
    let result = result.expect("a type name may directly precede its string literal");
    assert_eq!(result.rows(), &[vec![Value::Int64(42)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_apply_postgres_arithmetic_before_string_concatenation() {
    // Arrange
    let fixture = sql_fixture("issue-796-arithmetic-concatenation-precedence", &[]);

    // Act
    let result =
        fixture.execute("SELECT 'a' || 1 + 2 AS text_right_add, 1 + 2 || 'b' AS add_left_text");

    // Assert
    let result = result.expect("arithmetic binds more tightly than concatenation");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("a3".to_owned()),
            Value::String("3b".to_owned()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25]
    );
}

#[test]
fn should_parse_postgres_postfix_absolute_value_operator() {
    // Arrange
    let fixture = sql_fixture("issue-796-absolute-value-operator", &[]);

    // Act
    let result = fixture.execute("SELECT @ -7 AS value");

    // Assert
    assert_eq!(
        result.expect("prefix absolute-value operator").rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_apply_expression_collation_to_comparisons() {
    // Arrange
    let fixture = sql_fixture("issue-796-expression-collation-comparison", &[]);

    // Act
    let result = fixture.execute("SELECT 'A' COLLATE \"C\" = 'a' COLLATE \"C\"");

    // Assert
    assert_eq!(
        result.expect("collated expression comparison").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_expression_collation_to_ordering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-expression-collation-ordering",
        &[
            "CREATE TABLE collation_values (value TEXT)",
            "INSERT INTO collation_values (value) VALUES ('b'), ('A'), ('a')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value FROM collation_values ORDER BY value COLLATE \"C\"");

    // Assert
    assert_eq!(
        result.expect("collated expression ordering").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("a".to_owned())],
            vec![Value::String("b".to_owned())],
        ]
    );
}

#[test]
fn should_order_text_with_a_non_c_postgres_collation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-non-c-collation-ordering",
        &[
            "CREATE TABLE collation_words (word TEXT)",
            "INSERT INTO collation_words (word) VALUES ('z'), ('ä'), ('a')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT word FROM collation_words ORDER BY word COLLATE \"en-US-x-icu\"");

    // Assert
    assert_eq!(
        result.expect("ICU English collation ordering").rows(),
        &[
            vec![Value::String("a".to_owned())],
            vec![Value::String("ä".to_owned())],
            vec![Value::String("z".to_owned())],
        ]
    );
}

#[test]
fn should_create_a_nondeterministic_postgres_icu_collation() {
    // Arrange
    let fixture = sql_fixture("issue-796-nondeterministic-icu-collation", &[]);

    // Act
    fixture
        .execute(
            "CREATE COLLATION issue_796_ci (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
        )
        .expect("create a nondeterministic ICU collation");
    let result = fixture.execute("SELECT 'A' = 'a' COLLATE issue_796_ci");

    // Assert
    assert_eq!(
        result
            .expect("case-insensitive collation comparison")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_refresh_a_postgres_collation_version() {
    // Arrange
    let fixture = sql_fixture("issue-796-alter-collation-refresh", &[]);

    // Act
    let result = fixture.execute("ALTER COLLATION \"C\" REFRESH VERSION");

    // Assert
    result.expect("refresh the recorded version of a PostgreSQL collation");
}

#[test]
fn should_apply_column_collation_to_distinct_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-distinct",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT DISTINCT value FROM collated_values ORDER BY value");

    // Assert
    assert_eq!(
        result.expect("collation-aware DISTINCT").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("a".to_owned())],
        ]
    );
}

#[test]
fn should_apply_column_collation_to_group_keys() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-grouping",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value, COUNT(*) FROM collated_values GROUP BY value");

    // Assert
    assert_eq!(result.expect("collation-aware GROUP BY").rows().len(), 2);
}

#[test]
fn should_apply_column_collation_to_like_matching() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-column-collation-like",
        &[
            "CREATE TABLE collated_values (value TEXT COLLATE \"C\")",
            "INSERT INTO collated_values (value) VALUES ('A'), ('a'), ('A')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT value FROM collated_values WHERE value LIKE 'A'");

    // Assert
    assert_eq!(
        result.expect("collation-aware LIKE").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("A".to_owned())]
        ]
    );
}

#[test]
fn should_return_postgres_float_special_values() {
    // Arrange
    let fixture = sql_fixture("issue-796-float-special-values", &[]);

    // Act
    let cases: [(&str, ValuePredicate); 6] = [
        (
            "SELECT CAST('NaN' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_nan()),
        ),
        (
            "SELECT CAST('Infinity' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_positive()),
        ),
        (
            "SELECT CAST('-Infinity' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_negative()),
        ),
        (
            "SELECT CAST('inf' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_positive()),
        ),
        (
            "SELECT CAST('-inf' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_negative()),
        ),
        (
            "SELECT CAST('nan' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_nan()),
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, is_valid)| match fixture.execute(sql) {
            Ok(result)
                if result
                    .rows()
                    .first()
                    .and_then(|row| row.first())
                    .is_some_and(is_valid) =>
            {
                None
            }
            Ok(result) => Some(format!("{sql}: unexpected result {:?}", result.rows())),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "floating-point special values failed:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_return_boolean_input_validation_results_for_postgres_type_names() {
    // Arrange
    let fixture = sql_fixture("issue-796-input-validity", &[]);

    // Act
    let result = fixture
        .execute("SELECT pg_input_is_valid('42', 'integer'), pg_input_is_valid('bad', 'integer')");

    // Assert
    assert_eq!(
        result.expect("pg_input_is_valid results").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_return_postgres_input_error_details_without_raising_for_invalid_text() {
    // Arrange
    let fixture = sql_fixture("issue-796-input-error-info", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM pg_input_error_info('bad', 'integer')");

    // Assert
    let result = result.expect("pg_input_error_info reports conversion failure as data");
    assert_eq!(result.rows().len(), 1);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["message", "detail", "hint", "sql_error_code"]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("invalid input syntax for type integer: \"bad\"".to_owned()),
            Value::Null,
            Value::Null,
            Value::String("22P02".to_owned()),
        ]]
    );
}
