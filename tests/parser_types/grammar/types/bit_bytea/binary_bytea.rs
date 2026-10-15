//! PostgreSQL 18 bit-string and binary-string grammar tracked by #816.

use cassie::sql::ast::{Expr, FunctionCall, QueryStatement, SelectItem, SelectStatement};
use cassie::types::{DataType, Value};

use crate::support_sql_fixture::sql_fixture;

fn parsed_select(sql: &str) -> SelectStatement {
    let parsed = cassie::sql::parse_statement(sql).expect("SELECT statement parses");
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT statement");
    };
    *statement
}

fn projection_function(statement: &SelectStatement, index: usize) -> &FunctionCall {
    match &statement.projection[index] {
        SelectItem::Function { function, .. } => function,
        SelectItem::Expr { .. } => panic!("expected function projection"),
        SelectItem::Column { name, .. } => panic!("expected function, got column {name}"),
        SelectItem::WindowFunction { function, .. } => {
            panic!(
                "expected ordinary function, got window function {}",
                function.name
            )
        }
        SelectItem::Wildcard => panic!("expected function, got wildcard"),
    }
}

fn string_literal(expression: &Expr) -> &str {
    match expression {
        Expr::StringLiteral(value) => value,
        _ => panic!("expected string literal expression"),
    }
}

fn integer_literal(expression: &Expr) -> i64 {
    match expression {
        Expr::IntegerLiteral(value) => *value,
        _ => panic!("expected integer literal expression"),
    }
}

fn assert_bytea_literal(expression: &Expr, expected_value: &str) {
    let Expr::Cast {
        expr,
        data_type: DataType::Bytea,
    } = expression
    else {
        panic!("expected explicit bytea cast");
    };
    assert_eq!(string_literal(expr), expected_value);
}

fn assert_function_name(function: &FunctionCall, expected_name: &str) {
    assert_eq!(function.name.to_ascii_lowercase(), expected_name);
}

#[test]
fn should_parse_binary_plus_hex_bit_string_constants() {
    // Arrange
    let sql = "SELECT B'10101', b'001', X'1F', x'a0'";

    // Act
    let statement = parsed_select(sql);

    // Assert
    let expected_literals: [(&str, usize); 4] =
        [("10101", 5), ("001", 3), ("00011111", 8), ("10100000", 8)];
    assert_eq!(statement.projection.len(), expected_literals.len());
    let actual_literals = statement
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Expr {
                expr: Expr::StringLiteral(bits),
                ..
            } => Some((bits.clone(), bits.chars().count())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let expected_literals = expected_literals
        .iter()
        .map(|(bits, width)| Some(((*bits).to_string(), *width)))
        .collect::<Vec<_>>();
    assert_eq!(actual_literals, expected_literals);
}

#[test]
fn should_report_bit_type_modifiers_in_information_schema() {
    // Arrange
    let fixture = sql_fixture(
        "issue-816-bit-type-modifiers",
        &["CREATE TABLE issue_816_bits (fixed BIT(3), variable BIT VARYING, alias VARBIT(5))"],
    );

    // Act
    let result = fixture.execute(
        "SELECT column_name, data_type, character_maximum_length \
         FROM information_schema.columns \
         WHERE table_name = 'issue_816_bits' ORDER BY ordinal_position",
    );

    // Assert
    assert_eq!(
        result
            .expect("BIT typmods appear in information_schema")
            .rows(),
        &[
            vec![
                Value::String("fixed".to_string()),
                Value::String("bit".to_string()),
                Value::Int64(3),
            ],
            vec![
                Value::String("variable".to_string()),
                Value::String("bit varying".to_string()),
                Value::Null,
            ],
            vec![
                Value::String("alias".to_string()),
                Value::String("bit varying".to_string()),
                Value::Int64(5),
            ],
        ]
    );
}

#[test]
fn should_execute_bitwise_operator_forms_with_postgres_values() {
    // Arrange
    let fixture = sql_fixture("issue-816-bitwise-values", &[]);

    // Act
    let result = fixture.execute(
        "SELECT B'10001' || B'011', B'10001' & B'01101', B'10001' | B'01101', \
         B'10001' # B'01101', ~B'10001', B'10001' << 3, B'10001' >> 2",
    );

    // Assert
    let result = result.expect("bitwise expressions execute");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("10001011".to_string()),
            Value::String("00001".to_string()),
            Value::String("11101".to_string()),
            Value::String("11100".to_string()),
            Value::String("01110".to_string()),
            Value::String("01000".to_string()),
            Value::String("00100".to_string()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1560; 7]
    );
}

#[test]
fn should_execute_bit_string_functions_with_postgres_values() {
    // Arrange
    let fixture = sql_fixture("issue-816-bit-functions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT position(B'010' IN B'000001101011'), \
         substring(B'110010111111' FROM 3 FOR 2), \
         overlay(B'01010101010101010' PLACING B'11111' FROM 2 FOR 3), \
         get_bit(B'101010', 2), set_bit(B'101010', 2, 0)",
    );

    // Assert
    let result = result.expect("bit string functions execute");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(8),
            Value::String("00".to_string()),
            Value::String("0111110101010101010".to_string()),
            Value::Int64(1),
            Value::String("100010".to_string()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 1560, 1560, 23, 1560]
    );
}

#[test]
fn should_execute_bytea_operator_cast_forms_with_postgres_values() {
    // Arrange
    let fixture = sql_fixture("issue-816-bytea-values", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ('\\x1234'::bytea || '\\x00ff'::bytea)::text, \
         (1234::smallint::bytea)::text, ('\\x8000'::bytea::integer)::text",
    );

    // Assert
    assert_eq!(
        result.expect("bytea operators and casts execute").rows(),
        &[vec![
            Value::String("\\x123400ff".to_string()),
            Value::String("\\x04d2".to_string()),
            Value::String("32768".to_string()),
        ]]
    );
}

#[test]
fn should_preserve_bytea_substring_function_operands() {
    // Arrange
    let sql = "SELECT substring('\\x010203'::bytea FROM 2 FOR 1)";

    // Act
    let statement = parsed_select(sql);

    // Assert
    let substring = projection_function(&statement, 0);
    assert_function_name(substring, "substring");
    assert_eq!(substring.args.len(), 3);
    assert_bytea_literal(&substring.args[0], "\\x010203");
    assert_eq!(integer_literal(&substring.args[1]), 2);
    assert_eq!(integer_literal(&substring.args[2]), 1);
}

#[test]
fn should_preserve_bytea_overlay_function_operands() {
    // Arrange
    let sql = "SELECT overlay('\\x010203'::bytea PLACING '\\xff'::bytea FROM 2 FOR 1)";

    // Act
    let statement = parsed_select(sql);

    // Assert
    let overlay = projection_function(&statement, 0);
    assert_function_name(overlay, "overlay");
    assert_eq!(overlay.args.len(), 4);
    assert_bytea_literal(&overlay.args[0], "\\x010203");
    assert_bytea_literal(&overlay.args[1], "\\xff");
    assert_eq!(integer_literal(&overlay.args[2]), 2);
    assert_eq!(integer_literal(&overlay.args[3]), 1);
}

#[test]
fn should_preserve_bytea_position_function_operands() {
    // Arrange
    let sql = "SELECT position('\\x02'::bytea IN '\\x010203'::bytea)";

    // Act
    let statement = parsed_select(sql);

    // Assert
    let position = projection_function(&statement, 0);
    assert_function_name(position, "position");
    assert_eq!(position.args.len(), 2);
    assert_bytea_literal(&position.args[0], "\\x02");
    assert_bytea_literal(&position.args[1], "\\x010203");
}

#[test]
fn should_preserve_bytea_trim_function_operands() {
    // Arrange
    let sql = "SELECT trim(BOTH '\\x01'::bytea FROM '\\x010201'::bytea)";

    // Act
    let statement = parsed_select(sql);

    // Assert
    let trim = projection_function(&statement, 0);
    assert_function_name(trim, "trim");
    assert_eq!(trim.args.len(), 2);
    assert_bytea_literal(&trim.args[0], "\\x010201");
    assert_bytea_literal(&trim.args[1], "\\x01");
}

#[test]
fn should_parse_bytea_encoding_conversion_access_functions() {
    // Arrange
    let sql = "SELECT encode(decode('00ff', 'hex'), 'base64'), convert_to('café', 'UTF8'), convert_from(decode('636166c3a9', 'hex'), 'UTF8'), convert(decode('636166c3a9', 'hex'), 'UTF8', 'LATIN1'), get_byte('\\x0102'::bytea, 1), set_byte('\\x0102'::bytea, 1, 3), get_bit('\\x80'::bytea, 7), set_bit('\\x80'::bytea, 7, 0)";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert_eq!(statement.projection.len(), 8);

    let encode = projection_function(&statement, 0);
    assert_function_name(encode, "encode");
    assert_eq!(encode.args.len(), 2);
    let Expr::Function(decode) = &encode.args[0] else {
        panic!("encode input must retain its nested decode call");
    };
    assert_function_name(decode, "decode");
    assert_eq!(decode.args.len(), 2);
    assert_eq!(string_literal(&decode.args[0]), "00ff");
    assert_eq!(string_literal(&decode.args[1]), "hex");
    assert_eq!(string_literal(&encode.args[1]), "base64");

    let convert_to = projection_function(&statement, 1);
    assert_function_name(convert_to, "convert_to");
    assert_eq!(convert_to.args.len(), 2);
    assert_eq!(string_literal(&convert_to.args[0]), "café");
    assert_eq!(string_literal(&convert_to.args[1]), "UTF8");

    let convert_from = projection_function(&statement, 2);
    assert_function_name(convert_from, "convert_from");
    assert_eq!(convert_from.args.len(), 2);
    let Expr::Function(decode) = &convert_from.args[0] else {
        panic!("convert_from input must retain its nested decode call");
    };
    assert_function_name(decode, "decode");
    assert_eq!(decode.args.len(), 2);
    assert_eq!(string_literal(&decode.args[0]), "636166c3a9");
    assert_eq!(string_literal(&decode.args[1]), "hex");
    assert_eq!(string_literal(&convert_from.args[1]), "UTF8");

    let convert = projection_function(&statement, 3);
    assert_function_name(convert, "convert");
    assert_eq!(convert.args.len(), 3);
    let Expr::Function(decode) = &convert.args[0] else {
        panic!("convert input must retain its nested decode call");
    };
    assert_function_name(decode, "decode");
    assert_eq!(decode.args.len(), 2);
    assert_eq!(string_literal(&decode.args[0]), "636166c3a9");
    assert_eq!(string_literal(&decode.args[1]), "hex");
    assert_eq!(string_literal(&convert.args[1]), "UTF8");
    assert_eq!(string_literal(&convert.args[2]), "LATIN1");

    let get_byte = projection_function(&statement, 4);
    assert_function_name(get_byte, "get_byte");
    assert_eq!(get_byte.args.len(), 2);
    assert_bytea_literal(&get_byte.args[0], "\\x0102");
    assert_eq!(integer_literal(&get_byte.args[1]), 1);

    let set_byte = projection_function(&statement, 5);
    assert_function_name(set_byte, "set_byte");
    assert_eq!(set_byte.args.len(), 3);
    assert_bytea_literal(&set_byte.args[0], "\\x0102");
    assert_eq!(integer_literal(&set_byte.args[1]), 1);
    assert_eq!(integer_literal(&set_byte.args[2]), 3);

    let get_bit = projection_function(&statement, 6);
    assert_function_name(get_bit, "get_bit");
    assert_eq!(get_bit.args.len(), 2);
    assert_bytea_literal(&get_bit.args[0], "\\x80");
    assert_eq!(integer_literal(&get_bit.args[1]), 7);

    let set_bit = projection_function(&statement, 7);
    assert_function_name(set_bit, "set_bit");
    assert_eq!(set_bit.args.len(), 3);
    assert_bytea_literal(&set_bit.args[0], "\\x80");
    assert_eq!(integer_literal(&set_bit.args[1]), 7);
    assert_eq!(integer_literal(&set_bit.args[2]), 0);
}
