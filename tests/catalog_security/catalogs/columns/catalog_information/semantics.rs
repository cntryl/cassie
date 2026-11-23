#[test]
fn should_report_created_table_default_expression() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-table-default");

    // Act
    let defaults = fixture
        .execute(
            "SELECT column_name, column_default \
             FROM information_schema.columns \
             WHERE table_schema = 'issue_817_catalog_probe' \
               AND table_name = 'items' AND column_name = 'amount'",
        )
        .expect("read stored column default");

    // Assert
    assert_eq!(
        defaults.rows(),
        vec![vec![
            Value::String("amount".to_owned()),
            Value::String("1.25".to_owned()),
        ]]
    );
}

#[test]
fn should_render_precision_scale_numeric_default_as_sql_expression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-default-expression-rendering",
        &["CREATE TABLE default_rendering (amount NUMERIC(8, 2) DEFAULT 12.30)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let defaults = fixture
        .execute(
            "SELECT column_name, column_default FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'default_rendering' \
             ORDER BY column_name",
        )
        .expect("read column defaults");
    let catalog_defaults = fixture
        .execute(
            "SELECT adsrc FROM pg_catalog.pg_attrdef \
             WHERE adrelid = 'default_rendering'",
        )
        .expect("read PostgreSQL attribute default");

    // Assert
    assert_eq!(
        defaults.rows(),
        vec![vec![
            Value::String("amount".to_owned()),
            Value::String("12.30".to_owned()),
        ]]
    );
    assert_eq!(
        catalog_defaults.rows(),
        vec![vec![Value::String("12.30".to_owned())]]
    );
}

#[test]
fn should_escape_quotes_in_text_column_default_expression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-text-default-expression-rendering",
        &["CREATE TABLE text_default_rendering (note TEXT DEFAULT 'O''Reilly')"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let defaults = fixture
        .execute(
            "SELECT column_default FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'text_default_rendering' \
               AND column_name = 'note'",
        )
        .expect("read text default");
    let catalog_defaults = fixture
        .execute(
            "SELECT adsrc FROM pg_catalog.pg_attrdef \
             WHERE adrelid = 'text_default_rendering'",
        )
        .expect("read PostgreSQL attribute default");

    // Assert
    assert_eq!(
        defaults.rows(),
        vec![vec![Value::String("'O''Reilly'".to_owned())]]
    );
    assert_eq!(
        catalog_defaults.rows(),
        vec![vec![Value::String("'O''Reilly'".to_owned())]]
    );
}

#[test]
fn should_accept_pretty_flag_given_pg_get_expr_when_describing_a_default() {
    // Arrange
    let fixture = sql_fixture("issue-1486-pg-get-expr-pretty", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_expr('7', 0, true)")
        .expect("render a stored default with pretty flag");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("7".to_owned())]]
    );
}

#[test]
fn should_return_null_given_null_expression_when_calling_pg_get_expr() {
    // Arrange
    let fixture = sql_fixture("issue-1486-pg-get-expr-null", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_get_expr(NULL, 0, true)")
        .expect("render a NULL expression");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Null]]);
}

#[test]
fn should_reject_four_arguments_when_calling_pg_get_expr() {
    // Arrange
    let fixture = sql_fixture("issue-1486-pg-get-expr-extra-argument", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture.execute("SELECT pg_get_expr('7', 0, true, false)");

    // Assert
    assert_eq!(result.expect_err("reject an extra argument").sqlstate(), "42883");
}

