//! An unaliased cast of a column is named after the column (PostgreSQL 18.6),
//! which also decides what `ORDER BY <name>` sorts.
use crate::support_sql_fixture::{SqlFixture, sql_fixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE cast_names (e INT, f TEXT)",
            "INSERT INTO cast_names VALUES (2, 'b'), (10, 'a')",
        ],
    )
}

fn column_names(result: &cassie::executor::QueryResult) -> Vec<String> {
    result
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

#[test]
fn should_name_an_unaliased_column_cast_after_the_column() {
    // Arrange
    let fixture = fixture("cast-output-name");

    // Act
    let result =
        fixture.execute("SELECT e::text, CAST(f AS varchar), cast_names.e::bigint FROM cast_names");

    // Assert
    assert!(result.is_ok_and(|result| column_names(&result) == ["e", "f", "e"]));
}

#[test]
fn should_order_by_the_cast_output_when_its_name_is_used() {
    // Arrange
    let fixture = fixture("cast-output-order");

    // Act
    let by_text = fixture.rows("SELECT e::text FROM cast_names ORDER BY e");
    let by_number = fixture.rows("SELECT e::text AS shown FROM cast_names ORDER BY e");

    // Assert
    assert_eq!(
        by_text,
        vec![
            vec![Value::String("10".into())],
            vec![Value::String("2".into())]
        ]
    );
    assert_eq!(
        by_number,
        vec![
            vec![Value::String("2".into())],
            vec![Value::String("10".into())]
        ]
    );
}

#[test]
fn should_keep_an_explicit_alias_on_a_column_cast() {
    // Arrange
    let fixture = fixture("cast-output-explicit-alias");

    // Act
    let result = fixture.execute("SELECT e::text AS shown FROM cast_names");

    // Assert
    assert!(result.is_ok_and(|result| column_names(&result) == ["shown"]));
}

#[test]
fn should_name_unaliased_arithmetic_expression_column_question_mark() {
    // Arrange
    let fixture = fixture("unaliased-expression-question-mark");

    // Act
    let result = fixture
        .execute("SELECT 1 + 1")
        .expect("arithmetic expression");

    // Assert
    assert_eq!(column_names(&result), ["?column?"]);
    assert_eq!(result.columns()[0].type_oid, 23);
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_name_unaliased_casts_case_and_cast_function_expressions() {
    // Arrange
    let fixture = fixture("unaliased-expression-derived-names");

    // Act
    let numeric = fixture.execute("SELECT 1::numeric").expect("numeric cast");
    let integer = fixture.execute("SELECT CAST(1 AS int)").expect("int cast");
    let case = fixture
        .execute("SELECT CASE WHEN true THEN 1 END")
        .expect("case expression");
    let function_cast = fixture
        .execute("SELECT now()::date")
        .expect("function cast");
    let boolean = fixture.execute("SELECT true").expect("boolean literal");
    let negative = fixture.execute("SELECT -1").expect("negative literal");

    // Assert
    assert_eq!(column_names(&numeric), ["numeric"]);
    assert_eq!(numeric.columns()[0].type_oid, 1700);
    assert_eq!(column_names(&integer), ["int4"]);
    assert_eq!(integer.columns()[0].type_oid, 23);
    assert_eq!(column_names(&case), ["case"]);
    assert_eq!(column_names(&function_cast), ["now"]);
    assert_eq!(function_cast.columns()[0].type_oid, 1082);
    assert_eq!(column_names(&boolean), ["?column?"]);
    assert_eq!(column_names(&negative), ["?column?"]);
}

#[test]
fn should_name_unaliased_casts_with_postgresql_catalog_types_and_exists() {
    // Arrange
    let fixture = fixture("unaliased-expression-catalog-type-names");

    // Act
    let result = fixture
        .execute(
            "SELECT true::boolean, 'x'::char, 1::smallint, 1::integer, 1::bigint, \
             1::real, 1::double precision, 1::numeric, 'x'::text, 'x'::varchar, \
             '2020-01-01'::date, '12:00:00'::time, \
             '2020-01-01 12:00:00'::timestamp, '{}'::json, '{}'::jsonb, '$'::jsonpath, \
             '{1,2}'::int[], EXISTS (SELECT 1)",
        )
        .expect("catalog cast labels");

    // Assert
    assert_eq!(
        column_names(&result),
        [
            "bool",
            "bpchar",
            "int2",
            "int4",
            "int8",
            "float4",
            "float8",
            "numeric",
            "text",
            "varchar",
            "date",
            "time",
            "timestamp",
            "json",
            "jsonb",
            "jsonpath",
            "int4",
            "exists",
        ]
    );
}

#[test]
fn should_keep_function_names_aliases_and_duplicate_labels_positionally() {
    // Arrange
    let fixture = fixture("unaliased-expression-name-guards");

    // Act
    let functions = fixture
        .execute("SELECT abs(-1), round(1.5)")
        .expect("function labels");
    let alias = fixture
        .execute("SELECT 1 + 1 AS total")
        .expect("explicit alias");
    let duplicate = fixture
        .execute("SELECT 1 + 1, 2 + 2")
        .expect("duplicate expression labels");
    let cast_column = fixture
        .execute("SELECT CAST(e AS text) FROM cast_names")
        .expect("cast column label");

    // Assert
    assert_eq!(column_names(&functions), ["abs", "round"]);
    assert_eq!(column_names(&alias), ["total"]);
    assert_eq!(column_names(&duplicate), ["?column?", "?column?"]);
    assert_eq!(duplicate.rows(), &[vec![Value::Int64(2), Value::Int64(4)]]);
    assert_eq!(column_names(&cast_column), ["e"]);
}

#[test]
fn should_expose_duplicate_expression_labels_through_a_derived_table() {
    // Arrange
    let fixture = fixture("unaliased-expression-derived-table-labels");

    // Act
    let result = fixture
        .execute("SELECT * FROM (SELECT 1 + 1, 2 + 2) s")
        .expect("derived table expressions");

    // Assert
    assert_eq!(column_names(&result), ["?column?", "?column?"]);
    assert_eq!(result.rows(), &[vec![Value::Int64(2), Value::Int64(4)]]);
}
