//! PostgreSQL 18 UUID, hashing, and `pg_lsn` syntax tracked by #816.

use cassie::sql::ast::{BinaryOp, Expr, QueryStatement, SelectItem};
use cassie::sql::parse_statement;
use cassie::types::DataType;

fn select_projection(sql: &str) -> Vec<SelectItem> {
    let parsed = parse_statement(sql).expect("statement should parse");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT");
    };
    select.projection
}

fn function_projection<'a>(item: &'a SelectItem, expected_name: &str) -> &'a [Expr] {
    let SelectItem::Function { function, alias } = item else {
        panic!("expected function projection");
    };
    assert_eq!(function.name, expected_name);
    assert!(alias.is_none());
    &function.args
}

fn assert_uuid_cast(item: &SelectItem, expected_literal: &str) {
    let SelectItem::Expr { expr, alias } = item else {
        panic!("expected expression projection");
    };
    assert!(alias.is_none());
    assert_uuid_cast_expr(expr, expected_literal);
}

fn assert_uuid_cast_expr(expr: &Expr, expected_literal: &str) {
    let Expr::Cast {
        expr,
        data_type: DataType::Uuid,
    } = expr
    else {
        panic!("expected UUID cast expression");
    };
    let Expr::StringLiteral(literal) = expr.as_ref() else {
        panic!("expected string literal inside UUID cast");
    };
    assert_eq!(literal, expected_literal);
}

#[test]
fn should_parse_uuid_alternative_input_casts() {
    // Arrange
    let sql = "SELECT CAST('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11' AS UUID), '{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}'::UUID, 'a0eebc999c0b4ef8bb6d6bb9bd380a11'::UUID";

    // Act
    let projection = select_projection(sql);

    // Assert
    assert_eq!(projection.len(), 3);
    assert_uuid_cast(&projection[0], "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11");
    assert_uuid_cast(&projection[1], "{a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11}");
    assert_uuid_cast(&projection[2], "a0eebc999c0b4ef8bb6d6bb9bd380a11");
}

#[test]
fn should_parse_postgresql_uuid_generation_calls_as_generic_function_nodes() {
    // Arrange
    let sql = "SELECT gen_random_uuid(), uuidv4(), uuidv7()";

    // Act
    let projection = select_projection(sql);

    // Assert
    assert_eq!(projection.len(), 3);
    for (item, name) in projection
        .iter()
        .zip(["gen_random_uuid", "uuidv4", "uuidv7"])
    {
        assert!(function_projection(item, name).is_empty());
    }
}

#[test]
fn should_parse_postgresql_uuid_generation_extraction_calls() {
    // Arrange
    let sql = "SELECT gen_random_uuid(), uuidv4(), uuidv7(), uuid_extract_version(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e'), uuid_extract_timestamp(UUID '019535d9-3df7-79fb-b466-fa907fa17f9e')";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "UUID generation and extraction: {parsed:?}");
}

#[test]
fn should_preserve_uuid_comparison_as_binary_cast_nodes() {
    // Arrange
    let sql = "SELECT 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11'::UUID = 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'::UUID";

    // Act
    let projection = select_projection(sql);

    // Assert
    assert_eq!(projection.len(), 1);
    let SelectItem::Expr {
        expr:
            Expr::Binary {
                left,
                op: BinaryOp::Eq,
                right,
            },
        alias,
    } = &projection[0]
    else {
        panic!("expected UUID comparison binary expression");
    };
    assert!(alias.is_none());
    assert_uuid_cast_expr(left, "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11");
    assert_uuid_cast_expr(right, "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11");
}

#[test]
fn should_parse_pg_lsn_input_operators() {
    // Arrange
    let sql = "SELECT '16/B374D848'::pg_lsn < '16/B374D850'::pg_lsn, '16/B374D848'::pg_lsn + 8, '16/B374D850'::pg_lsn - '16/B374D848'::pg_lsn";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "pg_lsn operators: {parsed:?}");
}

#[test]
fn should_parse_pg_lsn_function_calls() {
    // Arrange
    let sql = "SELECT pg_wal_lsn_diff('16/B374D850'::pg_lsn, '16/B374D848'::pg_lsn), pg_lsn('16'::numeric)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "pg_lsn functions: {parsed:?}");
}

#[test]
fn should_preserve_pg_lsn_constructor_argument_cast_ast() {
    // Arrange
    let sql = "SELECT pg_lsn('16'::numeric)";

    // Act
    let projection = select_projection(sql);

    // Assert
    assert_eq!(projection.len(), 1);
    let [argument] = function_projection(&projection[0], "pg_lsn") else {
        panic!("expected one pg_lsn argument");
    };
    let Expr::Cast { expr, data_type } = argument else {
        panic!("expected generic cast argument");
    };
    assert_eq!(data_type, &DataType::Float);
    let Expr::StringLiteral(literal) = expr.as_ref() else {
        panic!("expected string input literal");
    };
    assert_eq!(literal, "16");
}

#[test]
fn should_parse_postgresql_hashing_calls() {
    // Arrange
    let sql = "SELECT md5('abc'), sha256('abc'::bytea), digest('abc', 'sha256'), hashtext('abc'), hashtextextended('abc', 0)";

    // Act
    let projection = select_projection(sql);

    // Assert
    assert_eq!(projection.len(), 5);
    let [argument] = function_projection(&projection[0], "md5") else {
        panic!("expected one md5 argument");
    };
    assert!(matches!(argument, Expr::StringLiteral(value) if value == "abc"));

    let [argument] = function_projection(&projection[1], "sha256") else {
        panic!("expected one sha256 argument");
    };
    let Expr::Cast { expr, data_type } = argument else {
        panic!("expected generic bytea cast argument");
    };
    assert_eq!(data_type, &DataType::Bytea);
    assert!(matches!(expr.as_ref(), Expr::StringLiteral(value) if value == "abc"));

    let [input, algorithm] = function_projection(&projection[2], "digest") else {
        panic!("expected two digest arguments");
    };
    assert!(matches!(input, Expr::StringLiteral(value) if value == "abc"));
    assert!(matches!(algorithm, Expr::StringLiteral(value) if value == "sha256"));

    let [argument] = function_projection(&projection[3], "hashtext") else {
        panic!("expected one hashtext argument");
    };
    assert!(matches!(argument, Expr::StringLiteral(value) if value == "abc"));

    let [input, seed] = function_projection(&projection[4], "hashtextextended") else {
        panic!("expected two hashtextextended arguments");
    };
    assert!(matches!(input, Expr::StringLiteral(value) if value == "abc"));
    assert!(matches!(seed, Expr::IntegerLiteral(0)));
}
