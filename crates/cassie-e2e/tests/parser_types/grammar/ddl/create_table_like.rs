use cassie::sql::ast::{
    CreateTableLikeFeature, CreateTableLikeOption, CreateTableStatement, QueryStatement,
};
use cassie::sql::IdentifierPath;
use cassie::sql::{parse_statement, SqlErrorKind};

#[test]
fn should_parse_create_table_like_with_ordered_options() {
    // Arrange
    let sql = "CREATE TABLE records_copy (LIKE records INCLUDING ALL EXCLUDING IDENTITY)";

    // Act
    let parsed = parse_statement(sql).expect("parse CREATE TABLE LIKE");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected CREATE TABLE statement");
    };
    assert!(statement.fields.is_empty());
    let like = statement.like.as_ref().expect("LIKE clause");
    assert_eq!(
        like.source,
        IdentifierPath::parse("records").expect("source table path")
    );
    assert_eq!(
        like.options,
        vec![
            CreateTableLikeOption::Including(CreateTableLikeFeature::All),
            CreateTableLikeOption::Excluding(CreateTableLikeFeature::Identity),
        ]
    );

    let serialized = serde_json::to_value(&statement).expect("serialize CREATE TABLE AST");
    let round_trip: CreateTableStatement =
        serde_json::from_value(serialized.clone()).expect("deserialize current CREATE TABLE AST");
    assert_eq!(round_trip.like.as_ref(), Some(like));

    let mut legacy_serialized = serialized;
    legacy_serialized
        .as_object_mut()
        .expect("serialized statement object")
        .remove("like");
    let legacy: CreateTableStatement =
        serde_json::from_value(legacy_serialized).expect("deserialize pre-LIKE AST");
    assert!(legacy.like.is_none());
}

#[test]
fn should_parse_every_postgresql_table_like_option() {
    // Arrange
    let sql = "CREATE TABLE copied (LIKE source \
        INCLUDING COMMENTS EXCLUDING COMPRESSION INCLUDING CONSTRAINTS \
        INCLUDING DEFAULTS INCLUDING GENERATED INCLUDING IDENTITY \
        INCLUDING INDEXES INCLUDING STATISTICS INCLUDING STORAGE)";

    // Act
    let parsed = parse_statement(sql).expect("parse table LIKE option vocabulary");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected CREATE TABLE statement");
    };
    assert_eq!(
        statement.like.expect("LIKE clause").options,
        vec![
            CreateTableLikeOption::Including(CreateTableLikeFeature::Comments),
            CreateTableLikeOption::Excluding(CreateTableLikeFeature::Compression),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Constraints),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Defaults),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Generated),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Identity),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Indexes),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Statistics),
            CreateTableLikeOption::Including(CreateTableLikeFeature::Storage),
        ]
    );
}

#[test]
fn should_parse_quoted_schema_qualified_like_source() {
    // Arrange
    let sql = "CREATE TABLE copied (LIKE public.\"source table\" INCLUDING ALL)";

    // Act
    let parsed = parse_statement(sql).expect("parse quoted schema-qualified source");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected CREATE TABLE statement");
    };
    assert_eq!(
        statement.like.expect("LIKE clause").source,
        IdentifierPath::parse("public.\"source table\"").expect("source table path")
    );
}

#[test]
fn should_reject_mixed_like_column_elements_in_this_slice() {
    // Arrange
    let sql = "CREATE TABLE records_copy (LIKE records INCLUDING ALL, extra INT)";

    // Act
    let error = parse_statement(sql).expect_err("mixed LIKE table elements are pending");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
    assert!(error.message().contains("single LIKE table element"));
}

#[test]
fn should_reject_multiple_like_clauses_in_this_slice() {
    // Arrange
    let sql = "CREATE TABLE records_copy (LIKE records, LIKE records_archive)";

    // Act
    let error = parse_statement(sql).expect_err("multiple LIKE clauses are pending");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
    assert!(error.message().contains("single LIKE table element"));
}

#[test]
fn should_reject_unknown_table_like_options() {
    // Arrange
    let sql = "CREATE TABLE records_copy (LIKE records INCLUDING FUTURE_OPTION)";

    // Act
    let error = parse_statement(sql).expect_err("unknown LIKE options must not be ignored");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
    assert!(error.message().contains("FUTURE_OPTION"));
}
