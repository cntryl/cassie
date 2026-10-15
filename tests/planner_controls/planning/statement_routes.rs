use cassie::sql::ast::{
    CatalogStatement, CatalogStatementRef, ProjectionStatement, ProjectionStatementRef,
    RetentionStatement, RetentionStatementRef, RuntimeStatement, RuntimeStatementRef,
    StatementFamily, StatementRoute, StatementRouteRef,
};
use cassie::sql::parser::parse_statement;

fn parsed_route(sql: &str) -> (StatementFamily, cassie::sql::ast::QueryStatement) {
    let parsed = parse_statement(sql).expect("statement should parse");
    (parsed.statement.family(), parsed.statement)
}

#[test]
fn should_preserve_owned_statement_routes_for_every_family() {
    // Arrange
    let runtime = parse_statement("SELECT 1")
        .expect("runtime statement")
        .statement;
    let catalog = parse_statement("CREATE TABLE route_docs (title TEXT)")
        .expect("catalog statement")
        .statement;
    let projection = parse_statement(
        "CREATE ROLLUP route_rollup ON route_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
    )
    .expect("projection statement")
    .statement;
    let retention = parse_statement(
        "CREATE RETENTION POLICY route_retention ON route_events USING event_at RETAIN FOR '7 days'",
    )
    .expect("retention statement")
    .statement;

    // Act
    let routes = [
        runtime.into_route(),
        catalog.into_route(),
        projection.into_route(),
        retention.into_route(),
    ];

    // Assert
    assert!(matches!(
        routes[0],
        StatementRoute::Runtime(RuntimeStatement::Select(_))
    ));
    assert!(matches!(
        routes[1],
        StatementRoute::Catalog(CatalogStatement::CreateTable(_))
    ));
    assert!(matches!(
        routes[2],
        StatementRoute::Projection(ProjectionStatement::CreateRollup(_))
    ));
    assert!(matches!(
        routes[3],
        StatementRoute::Retention(RetentionStatement::CreateRetentionPolicy(_))
    ));
}

#[test]
fn should_route_select_statements_through_runtime_family() {
    // Arrange

    // Act
    let (family, statement) = parsed_route("SELECT title FROM route_docs");

    // Assert
    assert_eq!(family, StatementFamily::Runtime);
    assert!(matches!(
        statement.route(),
        StatementRouteRef::Runtime(RuntimeStatementRef::Select(_))
    ));
}

#[test]
fn should_route_create_table_statements_through_catalog_family() {
    // Arrange

    // Act
    let (family, statement) = parsed_route("CREATE TABLE route_docs (title TEXT)");

    // Assert
    assert_eq!(family, StatementFamily::Catalog);
    assert!(matches!(
        statement.route(),
        StatementRouteRef::Catalog(CatalogStatementRef::CreateTable(_))
    ));
}

#[test]
fn should_route_projection_statements_through_projection_family() {
    // Arrange

    // Act
    let (family, statement) = parsed_route(
        "CREATE ROLLUP route_rollup ON route_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
    );

    // Assert
    assert_eq!(family, StatementFamily::Projection);
    assert!(matches!(
        statement.route(),
        StatementRouteRef::Projection(ProjectionStatementRef::CreateRollup(_))
    ));
}

#[test]
fn should_route_retention_statements_through_retention_family() {
    // Arrange

    // Act
    let (family, statement) = parsed_route(
        "CREATE RETENTION POLICY route_retention ON route_events USING event_at RETAIN FOR '7 days'",
    );

    // Assert
    assert_eq!(family, StatementFamily::Retention);
    assert!(matches!(
        statement.route(),
        StatementRouteRef::Retention(RetentionStatementRef::CreateRetentionPolicy(_))
    ));
}

#[test]
fn should_route_transaction_statements_through_runtime_family() {
    // Arrange

    // Act
    let (family, statement) = parsed_route("BEGIN");

    // Assert
    assert_eq!(family, StatementFamily::Runtime);
    assert!(matches!(
        statement.route(),
        StatementRouteRef::Runtime(RuntimeStatementRef::Transaction(_))
    ));
}
