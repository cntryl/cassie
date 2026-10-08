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
        "{result:?}"
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
    assert!(result.is_ok(), "{result:?}");
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
            "{kind}: {result:?}"
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
            "{result:?}"
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
        "selected durable carrier results: {results:?}"
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
        "{result:?}"
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
        "all durable default writers must reject before publication: {results:?}"
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
        "{result:?}"
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
        "{result:?}"
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
        "{result:?}"
    );
}
