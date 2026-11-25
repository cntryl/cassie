use cassie::sql::{ast::QueryStatement, parse_statement};

fn select_projection(sql: &str) -> String {
    let parsed = parse_statement(sql).expect("selected dialect expression should parse");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT");
    };
    format!("{:?}", select.projection)
}

#[test]
fn should_parse_compact_comparisons_like_spaced_comparisons() {
    // Arrange
    let operators = ["=", "!=", "<>", "<", ">", "<=", ">="];

    // Act
    // Assert
    for operator in operators {
        let compact = select_projection(&format!("SELECT n{operator}$1 FROM records"));
        let spaced = select_projection(&format!("SELECT n {operator} $1 FROM records"));
        assert_eq!(compact, spaced, "operator {operator}");
    }
}

#[test]
fn should_parse_compact_arithmetic_with_identical_precedence() {
    // Arrange
    let expressions = [
        ("n+2*3", "n + 2 * 3"),
        ("n-2-3", "n - 2 - 3"),
        ("n/2*3", "n / 2 * 3"),
        ("n+-2", "n + -2"),
        ("n- -2", "n - -2"),
        ("n/+2", "n / +2"),
        ("n*-2", "n * -2"),
        ("n+1e-2", "n + 1e-2"),
    ];

    // Act
    // Assert
    for (compact, spaced) in expressions {
        assert_eq!(
            select_projection(&format!("SELECT {compact} FROM records")),
            select_projection(&format!("SELECT {spaced} FROM records")),
            "expression {compact}"
        );
    }
}

#[test]
fn should_preserve_operator_lexical_boundaries() {
    // Arrange
    let compact = "SELECT 'a>=b', \"n+2\", n/* nested /* > */ */>2 FROM records";
    let spaced = "SELECT 'a>=b', \"n+2\", n > 2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_preserve_operator_operand_comment_separators() {
    // Arrange
    let compact = "SELECT n/* > + */<=/* < - */2, n+/* nested /* + */ */-2 FROM records";
    let spaced = "SELECT n <= 2, n + -2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_preserve_line_comments_at_operator_boundaries() {
    // Arrange
    let compact = "SELECT n-- ignored - +\n+2 FROM records";
    let spaced = "SELECT n + 2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_reject_malformed_operator_sequences() {
    // Arrange
    let inputs = [
        "n><2",
        "n=>2",
        "n===2",
        "n+",
        "n/",
        "n>=$",
        "n < =2",
        "n! =2",
        "n**2",
        "n+*2",
        "abs(n)+*2",
        "1e-+2",
        "1e+-2",
        "1e-",
        "1e+",
    ];

    // Act
    let parsed =
        inputs.map(|expression| parse_statement(&format!("SELECT {expression} FROM records")));

    // Assert
    for (expression, result) in inputs.into_iter().zip(parsed) {
        assert!(result.is_err(), "invalid operator sequence {expression}");
    }
}

#[test]
fn should_reject_malformed_exponents_before_column_binding() {
    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "invalid_exponent_column",
        &[
            "CREATE TABLE records (\"1e\" BIGINT)",
            "INSERT INTO records (\"1e\") VALUES (5)",
        ],
    );

    // Act
    let query = fixture.execute("SELECT 1e-+2 FROM records");
    let parser_error = parse_statement("SELECT 1e-+2 FROM records")
        .expect_err("malformed numeric token must fail parsing");

    // Assert
    assert!(
        query.is_err(),
        "malformed exponent must not reference column 1e"
    );
    assert_eq!(parser_error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert_eq!(
        fixture.rows("SELECT \"1e\" FROM records"),
        vec![vec![cassie::types::Value::Int64(5)]]
    );
}

#[test]
fn should_keep_complete_operator_expressions_in_function_arguments() {
    use cassie::sql::ast::{BinaryOp, Expr, SelectItem};

    // Arrange
    let sql = "SELECT coalesce(abs(n)+2, 0) FROM records";

    // Act
    let parsed = parse_statement(sql).expect("operator function argument should parse");

    // Assert
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT");
    };
    let SelectItem::Function { function, .. } = &select.projection[0] else {
        panic!("expected function projection");
    };
    assert!(matches!(
        function.args[0],
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ));
}

#[test]
fn should_preserve_exponent_signs_at_arithmetic_boundaries() {
    // Arrange
    let compact = "SELECT 1e-2+2e+3 FROM records";
    let spaced = "SELECT 1e-2 + 2e+3 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_distinguish_exponent_like_identifiers_from_numeric_literals() {
    // Arrange
    let compact = "SELECT n1e-2 FROM records";
    let spaced = "SELECT n1e - 2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_preserve_multibyte_identifier_operator_boundaries() {
    // Arrange
    let compact = "SELECT é1e-2 FROM records";
    let spaced = "SELECT é1e - 2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_preserve_combining_mark_identifier_operator_boundaries() {
    // Arrange
    let compact = "SELECT e\u{301}1e-2 FROM records";
    let spaced = "SELECT e\u{301}1e - 2 FROM records";

    // Act
    let compact_projection = select_projection(compact);
    let spaced_projection = select_projection(spaced);

    // Assert
    assert_eq!(compact_projection, spaced_projection);
}

#[test]
fn should_parse_user_defined_operator_tokens_before_operator_resolution() {
    // Arrange
    let expressions = [
        ("n->'x'", true),
        ("n~'x'", true),
        ("n@>2", true),
        ("n?=2", false),
        ("n~+2", true),
        ("n|+2", false),
        ("n#>2", true),
        ("n<=>2", false),
    ];

    // Act
    let parsed = expressions
        .map(|(expression, _)| parse_statement(&format!("SELECT {expression} FROM records")));

    // Assert
    for ((expression, expected), result) in expressions.into_iter().zip(parsed) {
        assert_eq!(result.is_ok(), expected, "{expression}");
    }
}

#[test]
fn should_execute_compact_bound_operators_with_null_propagation() {
    use cassie::types::Value;

    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "compact_bound_operators",
        &[
            "CREATE TABLE records (n BIGINT)",
            "INSERT INTO records (n) VALUES (-2), (0), (3), (NULL)",
        ],
    );
    let compact =
        "SELECT n=$1,n!=$1,n<>$1,n<$1,n>$1,n<=$1,n>=$1,n+2*3 FROM records ORDER BY n NULLS LAST";
    let spaced = "SELECT n = $1,n != $1,n <> $1,n < $1,n > $1,n <= $1,n >= $1,n + 2 * 3 FROM records ORDER BY n NULLS LAST";

    // Act
    let compact = fixture
        .cassie
        .execute_sql(&fixture.session, compact, vec![Value::Int64(0)])
        .expect("compact bound query");
    let spaced = fixture
        .cassie
        .execute_sql(&fixture.session, spaced, vec![Value::Int64(0)])
        .expect("spaced bound query");

    // Assert
    assert_eq!(compact.columns(), spaced.columns());
    assert_eq!(compact.rows(), spaced.rows());
    let expected = [
        ([false, true, true, true, false, true, false], 4),
        ([true, false, false, false, false, true, true], 6),
        ([false, true, true, false, true, false, true], 9),
    ];
    for (row, (predicates, number)) in compact.rows().iter().zip(expected) {
        let mut values: Vec<_> = predicates.into_iter().map(Value::Bool).collect();
        values.push(Value::Int64(number));
        assert_eq!(row, &values);
    }
    assert_eq!(compact.rows().len(), 4);
    assert_eq!(compact.rows()[3], vec![Value::Null; 8]);
}

#[test]
fn should_preserve_compound_operator_names_given_adjacent_signs_when_scalar_tokens_are_scanned() {
    // Arrange
    let expressions = ["10%-3", "2^-3", "'x'||-2"];
    // Act
    let parsed = expressions.map(|expression| parse_statement(&format!("SELECT {expression}")));
    // Assert
    assert!(parsed.into_iter().all(|result| result.is_err()));
}

#[test]
fn should_reject_unselected_money_given_type_positions_when_parsing_read_model_sql() {
    // Arrange
    let statements = [
        "CREATE TABLE money_scope (amount money)",
        "SELECT '12.34'::money",
        "SELECT CAST('12.34' AS money)",
    ];
    // Act
    let parsed = statements.map(parse_statement);
    // Assert
    for (sql, result) in statements.into_iter().zip(parsed) {
        assert!(
            result
                .expect_err(sql)
                .to_string()
                .contains("unsupported data type"),
            "{sql}"
        );
    }
}
