use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;

#[test]
fn should_reject_persisted_view_null_safe_predicates_before_publication() {
    // Arrange
    let fixture = sql_fixture("null_safe_view_guard", &[]);
    // Act
    let result = fixture.execute(
        "CREATE VIEW forbidden_null_safe AS SELECT NULL IS NOT DISTINCT FROM NULL AS same",
    );
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "should_reject_persisted_view_null_safe_predicates_before_publication should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    assert!(fixture
        .execute("SELECT * FROM forbidden_null_safe")
        .is_err());
}

#[test]
fn should_preserve_keyword_text_in_existing_view_definitions() {
    // Arrange
    let fixture = sql_fixture("null_safe_view_literal", &[]);
    // Act
    let result = fixture
        .execute("CREATE VIEW allowed_keyword_text AS SELECT 'IS NOT DISTINCT FROM' AS text");
    // Assert
    assert!(result.is_ok(), "should_preserve_keyword_text_in_existing_view_definitions should report successful keyword-text definition");
    assert_eq!(
        fixture.rows("SELECT text FROM allowed_keyword_text"),
        vec![vec![cassie::types::Value::String(
            "IS NOT DISTINCT FROM".into()
        )]]
    );
}

#[test]
fn should_reject_direct_durable_null_safe_metadata_admission() {
    // Arrange
    let fixture = sql_fixture("null_safe_direct_metadata", &[]);
    let function = cassie::catalog::FunctionMeta {
        name: "forbidden_function".into(),
        args: vec![],
        return_type: cassie::types::DataType::Boolean,
        volatility: cassie::catalog::Volatility::Immutable,
        body: "NULL IS NOT DISTINCT FROM NULL".into(),
    };
    let procedure = cassie::catalog::ProcedureMeta {
        name: "forbidden_procedure".into(),
        args: vec![],
        body: "SELECT NULL IS DISTINCT FROM 1 AS different".into(),
    };
    let view = cassie::catalog::ViewMeta::new(
        "forbidden_direct_view",
        "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
        cassie::types::Schema { fields: vec![] },
    );
    // Act
    let results = [
        fixture.cassie.midge.put_function(&function),
        fixture.cassie.midge.put_procedure(&procedure),
        fixture.cassie.midge.put_view(&view),
    ];
    // Assert
    for (kind, result) in ["function", "procedure", "view"].iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
            "{kind}: should_reject_direct_durable_null_safe_metadata_admission should report Unsupported (SQLSTATE 0A000) for persisted definition"
        );
    }
    assert!(fixture
        .cassie
        .midge
        .get_function(&function.name)
        .expect("read function")
        .is_none());
    assert!(fixture
        .cassie
        .midge
        .get_procedure(&procedure.name)
        .expect("read procedure")
        .is_none());
    assert!(fixture
        .cassie
        .midge
        .get_view(&view.name)
        .expect("read view")
        .is_none());
}

#[test]
fn should_reject_sql_routine_null_safe_definitions() {
    // Arrange
    let fixture = sql_fixture("null_safe_sql_routines", &[]);
    // Act
    let results = [
        fixture.execute(r#"CREATE FUNCTION forbidden_sql_function() RETURNS BOOLEAN AS "NULL IS NOT DISTINCT FROM NULL""#),
        fixture.execute(r#"CREATE PROCEDURE forbidden_sql_procedure() AS "SELECT NULL IS DISTINCT FROM 1 AS different""#),
    ];
    // Assert
    for result in results {
        assert!(
            matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
            "should_reject_sql_routine_null_safe_definitions should report Unsupported (SQLSTATE 0A000) for persisted definition"
        );
    }
}

#[test]
fn should_reject_remaining_sql_null_safe_definition_carriers() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_other_definitions",
        &["CREATE TABLE definition_source (n BIGINT, tenant TEXT, event_at TIMESTAMP)"],
    );
    let statements = [
        "CREATE INDEX forbidden_predicate ON definition_source(n) WHERE n IS NOT DISTINCT FROM NULL",
        "CREATE MATERIALIZED PROJECTION forbidden_projection AS SELECT n FROM definition_source WHERE n IS DISTINCT FROM NULL",
        "CREATE ROLLUP forbidden_rollup ON definition_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total WHERE n IS DISTINCT FROM NULL",
        "CREATE TABLE forbidden_check (n BIGINT CHECK (n IS DISTINCT FROM NULL))",
        "CREATE TABLE forbidden_default (n BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))",
    ];
    // Act
    let results = statements
        .iter()
        .map(|sql| fixture.execute(sql))
        .collect::<Vec<_>>();
    // Assert
    assert!(
        results.iter().all(|result| matches!(result,
        Err(CassieError::Unsupported(message)) if message.contains("persisted definitions"))),
        "should_reject_remaining_sql_null_safe_definition_carriers should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
}

#[test]
fn should_reject_null_safe_predicates_in_nested_procedure_definitions() {
    // Arrange
    let fixture = sql_fixture("null_safe_nested_definition", &[]);
    let procedure = cassie::catalog::ProcedureMeta {
        name: "forbidden_nested_procedure".into(),
        args: vec![],
        body: "CREATE VIEW forbidden_nested_view AS SELECT NULL IS NOT DISTINCT FROM NULL AS same"
            .into(),
    };
    // Act
    let result = fixture.cassie.midge.put_procedure(&procedure);
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "should_reject_null_safe_predicates_in_nested_procedure_definitions should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    assert!(fixture
        .cassie
        .midge
        .get_procedure(&procedure.name)
        .expect("read procedure")
        .is_none());
}

#[test]
fn should_reject_direct_default_expression_publication() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_direct_defaults",
        &["CREATE TABLE default_source (n BOOLEAN)"],
    );
    let collection =
        crate::support_sql::canonical_test_collection(&fixture.cassie, "default_source");
    let mut constraint = cassie::catalog::FieldConstraint::new("n");
    constraint.default_expression = Some("NULL IS NOT DISTINCT FROM NULL".into());
    // Act
    let results = [
        fixture
            .cassie
            .midge
            .save_constraints(&collection, &[constraint.clone()]),
        fixture
            .cassie
            .midge
            .save_constraints_with_unique_reservations(&collection, &[constraint.clone()]),
        fixture
            .cassie
            .midge
            .save_constraints_and_release_unique_reservations(
                &collection,
                &[constraint],
                &["n".into()],
            ),
    ];
    // Assert
    assert!(
        results.iter().all(|result| matches!(result,
        Err(CassieError::Unsupported(message)) if message.contains("persisted definitions"))),
        "should_reject_direct_default_expression_publication should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    assert!(fixture
        .cassie
        .midge
        .load_constraints(&collection)
        .expect("unchanged constraints")
        .iter()
        .all(|constraint| constraint.default_expression.is_none()));
}

#[test]
fn should_reject_null_safe_arguments_in_stored_procedure_calls() {
    // Arrange
    let fixture = sql_fixture("null_safe_nested_call", &[]);
    let metadata = cassie::catalog::ProcedureMeta {
        name: "forbidden_nested_call".into(),
        args: vec![],
        body: "CALL other_procedure(CAST(NULL IS DISTINCT FROM 1 AS BOOLEAN))".into(),
    };
    // Act
    let result = fixture.cassie.midge.put_procedure(&metadata);
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "should_reject_null_safe_arguments_in_stored_procedure_calls should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    assert!(fixture
        .cassie
        .midge
        .get_procedure(&metadata.name)
        .expect("read denied metadata")
        .is_none());
}

#[test]
fn should_reject_loaded_default_expression_use() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_loaded_default",
        &["CREATE TABLE loaded_default (id BIGINT,n BOOLEAN)"],
    );
    let collection =
        crate::support_sql::canonical_test_collection(&fixture.cassie, "loaded_default");
    let mut constraint = cassie::catalog::FieldConstraint::new("n");
    constraint.default_expression = Some("NULL IS NOT DISTINCT FROM NULL".into());
    fixture
        .cassie
        .catalog
        .register_constraints(&collection, vec![constraint]);
    // Act
    let result = fixture.execute("INSERT INTO loaded_default(id) VALUES(1)");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "should_reject_loaded_default_expression_use should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM loaded_default"),
        Vec::<Vec<cassie::types::Value>>::new()
    );
}

#[test]
fn should_reject_loaded_index_predicate_use() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_loaded_index",
        &[
            "CREATE TABLE loaded_index (id BIGINT,n BIGINT)",
            "INSERT INTO loaded_index VALUES(1,7)",
            "CREATE INDEX loaded_n ON loaded_index(n)",
        ],
    );
    let collection = crate::support_sql::canonical_test_collection(&fixture.cassie, "loaded_index");
    let mut index = fixture
        .cassie
        .catalog
        .list_indexes(&collection)
        .into_iter()
        .find(|index| index.kind == cassie::catalog::IndexKind::Scalar)
        .expect("existing scalar index");
    let parsed = cassie::sql::parse_statement("SELECT n IS NOT DISTINCT FROM NULL AS same")
        .expect("actual predicate AST");
    let cassie::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT AST")
    };
    let cassie::sql::ast::SelectItem::Expr { expr, .. } =
        select.projection.into_iter().next().expect("one predicate")
    else {
        panic!("predicate Expr")
    };
    index.predicate = Some(serde_json::to_string(&expr).expect("existing Expr carrier"));
    fixture.cassie.catalog.register_index(index);
    // Act
    let result = fixture.execute("SELECT id FROM loaded_index WHERE n=7");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "should_reject_loaded_index_predicate_use should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
}

#[test]
fn should_reject_nested_default_parser_guard_before_procedure_publication() {
    // Arrange
    let fixture = sql_fixture("null_safe_nested_default_parser", &[]);
    let procedure = cassie::catalog::ProcedureMeta {
        name: "forbidden_parser_default".into(),
        args: vec![],
        body: "CREATE TABLE nested_default (n BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))"
            .into(),
    };
    let parser_error =
        cassie::sql::parse_statement(&procedure.body).expect_err("selected parser guard");
    assert_eq!(parser_error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(parser_error.message().contains("persisted definitions"));

    // Act
    let result = fixture.cassie.midge.put_procedure(&procedure);
    let published = fixture
        .cassie
        .midge
        .get_procedure(&procedure.name)
        .expect("read procedure");

    // Assert
    eprintln!("nested DEFAULT procedure published={}", published.is_some());
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "nested DEFAULT must preserve selected Unsupported rejection"
    );
    assert!(published.is_none(), "rejected procedure must not publish");
}

#[test]
fn should_reject_nested_check_parser_guard_before_procedure_publication() {
    // Arrange
    let fixture = sql_fixture("null_safe_nested_check_parser", &[]);
    let procedure = cassie::catalog::ProcedureMeta {
        name: "forbidden_parser_check".into(),
        args: vec![],
        body: "CREATE TABLE nested_check (n BIGINT CHECK (n IS DISTINCT FROM NULL))".into(),
    };
    let parser_error =
        cassie::sql::parse_statement(&procedure.body).expect_err("selected parser guard");
    assert_eq!(parser_error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(parser_error.message().contains("persisted definitions"));

    // Act
    let result = fixture.cassie.midge.put_procedure(&procedure);
    let published = fixture
        .cassie
        .midge
        .get_procedure(&procedure.name)
        .expect("read procedure");

    // Assert
    eprintln!("nested CHECK procedure published={}", published.is_some());
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "nested CHECK must preserve selected Unsupported rejection"
    );
    assert!(published.is_none(), "rejected procedure must not publish");
}

#[test]
fn should_preserve_unrelated_procedure_parser_admission() {
    // Arrange
    let fixture = sql_fixture("null_safe_legacy_parser_admission", &[]);
    let cases = [
        (
            "legacy_malformed",
            "CREATE TABLE unfinished (",
            cassie::sql::SqlErrorKind::Syntax,
        ),
        (
            "legacy_unsupported",
            "VACUUM absent",
            cassie::sql::SqlErrorKind::Unsupported,
        ),
    ];
    for (name, body, kind) in cases {
        let parser_error = cassie::sql::parse_statement(body).expect_err("legacy parser rejection");
        assert_eq!(parser_error.kind(), kind);
        let procedure = cassie::catalog::ProcedureMeta {
            name: name.into(),
            args: vec![],
            body: body.into(),
        };
        // Act
        let result = fixture.cassie.midge.put_procedure(&procedure);
        let published = fixture
            .cassie
            .midge
            .get_procedure(name)
            .expect("read legacy procedure");
        // Assert
        assert!(
            result.is_ok(),
            "unrelated parser diagnostics remain owned by existing execution callers"
        );
        assert_eq!(
            published.expect("legacy admission remains unchanged").body,
            body
        );
    }
}

#[test]
fn should_reject_recursive_procedure_parser_guard() {
    // Arrange
    let fixture = sql_fixture("null_safe_recursive_parser_guard", &[]);
    let procedure = cassie::catalog::ProcedureMeta {
        name: "forbidden_recursive_parser".into(), args: vec![],
        body: r#"CREATE PROCEDURE inner_guard() AS "CREATE TABLE nested_guard (n BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))""#.into(),
    };
    assert!(
        cassie::sql::parse_statement(&procedure.body).is_ok(),
        "outer routine syntax is supported"
    );
    // Act
    let result = fixture.cassie.midge.put_procedure(&procedure);
    let published = fixture
        .cassie
        .midge
        .get_procedure(&procedure.name)
        .expect("read recursive procedure");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "recursive procedure must report the selected Unsupported rejection"
    );
    assert!(
        published.is_none(),
        "rejected recursive procedure must not publish"
    );
}
