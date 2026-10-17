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
fn should_preserve_function_named_operator_given_qualified_identifier_when_expression_is_parsed() {
    // Arrange
    let sql = "SELECT public.operator(2) AS v";
    // Act
    let parsed = cassie::sql::parser::parse_statement(sql);
    // Assert
    let statement = parsed.expect("qualified function name must remain a function call");
    let cassie::sql::ast::QueryStatement::Select(select) = statement.statement else {
        panic!("SELECT");
    };
    let cassie::sql::ast::SelectItem::Function { function, .. } = &select.projection[0] else {
        panic!("function projection");
    };
    assert_eq!(function.name, "public.operator");
    assert_eq!(function.args.len(), 1);
}

#[test]
fn should_preserve_qualified_operator_function_names_given_lexical_variants_when_calls_are_parsed() {
    // Arrange
    let calls = [
        "public.operator(2)", "public.OPERATOR(2)", "\"public\".operator(2)",
        "public . operator(2)", "public /* schema */ . /* member */ operator /* args */ (2)",
        "public.\"operator\"(2)", "\"public\".\"operator\"(2)",
    ];
    // Act
    let results = calls.map(|call| cassie::sql::parser::parse_statement(&format!("SELECT {call} AS v")));
    // Assert
    for (call, result) in calls.iter().zip(results) {
        let parsed = result.unwrap_or_else(|error| panic!("{call}: {error}"));
        let cassie::sql::ast::QueryStatement::Select(select) = parsed.statement else { panic!("SELECT") };
        let cassie::sql::ast::SelectItem::Function { function, alias } = &select.projection[0] else { panic!("ordinary function: {call}") };
        assert_eq!(function.args.len(), 1);
        assert_eq!(alias.as_deref(), Some("v"));
    }
}

#[test]
fn should_preserve_operator_named_columns_given_arithmetic_when_keyword_lookahead_is_used() {
    // Arrange
    let fixture = sql_fixture("qualified-operator-column-control", &[]);
    let sql = [
        "WITH q(operator) AS (VALUES (2)) SELECT 1 + operator + 3 AS v FROM q",
        "WITH q(operator) AS (VALUES (2)) SELECT q.operator + 3 AS v FROM q",
    ];
    // Act
    let results = sql.map(|sql| fixture.execute(sql));
    // Assert
    for (result, value) in results.into_iter().zip([6, 5]) {
        let result = result.expect("operator remains an ordinary column name");
        assert_eq!(result.columns()[0].type_oid, 23);
        assert_eq!(result.rows(), &[vec![Value::Int64(value)]]);
    }
}

#[test]
fn should_preserve_quoted_parentheses_given_column_identifiers_when_function_calls_are_distinguished() {
    // Arrange
    let fixture = sql_fixture("qualified-operator-quoted-parentheses", &[]);
    let statements = [
        "WITH q(\"operator(2)\") AS (VALUES (4)) SELECT \"operator(2)\" AS v FROM q",
        "WITH q(\"operator(2)\") AS (VALUES (4)) SELECT q.\"operator(2)\" OPERATOR(pg_catalog.+) 1 AS v FROM q",
        "SELECT \"public\".\"operator(2)\" AS v FROM (VALUES (4)) AS \"public\"(\"operator(2)\")",
    ];
    // Act
    let results = statements.map(|sql| fixture.execute(sql));
    // Assert
    for (result, value) in results.into_iter().zip([4, 5, 4]) {
        let result = result.expect("quoted parentheses remain part of the identifier");
        assert_eq!(result.columns()[0].type_oid, 23);
        assert_eq!(result.rows(), &[vec![Value::Int64(value)]]);
    }
}

#[test]
fn should_preserve_operator_named_schema_components_given_function_calls_when_names_are_qualified() {
    // Arrange
    let calls = ["operator.f(2)", "\"operator\".f(2)", "operator.operator(2)", "\"operator(2)\".f(2)", "public.\"operator(2)\"(2)"];
    // Act
    let results = calls.map(|call| cassie::sql::parser::parse_statement(&format!("SELECT {call} AS v")));
    // Assert
    for (call, result) in calls.iter().zip(results) {
        let parsed = result.unwrap_or_else(|error| panic!("{call}: {error}"));
        let cassie::sql::ast::QueryStatement::Select(select) = parsed.statement else { panic!("SELECT") };
        let cassie::sql::ast::SelectItem::Function { function, alias } = &select.projection[0] else { panic!("ordinary function: {call}") };
        assert_eq!(function.args.len(), 1);
        assert_eq!(alias.as_deref(), Some("v"));
    }
}
