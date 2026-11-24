//! Native CARDINALITY counts elements without rendering their values.

use crate::query_correctness_fixture::started;
use cassie::types::Value;

const BIG: &str = "(9*(10::numeric^131071))";

#[test]
fn should_count_large_native_numeric_values_with_the_default_budget() {
    // Arrange
    let fixture = started("cardinality-native-numeric", &[]);
    let sql = format!("SELECT cardinality(ARRAY[{BIG},{BIG}])");
    // Act
    let result = fixture
        .execute(&sql)
        .expect("count native values without decimal scratch");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_count_large_numeric_values_through_the_pg_catalog_builtin() {
    // Arrange
    let fixture = started("cardinality-qualified-native-numeric", &[]);
    let sql = format!("SELECT pg_catalog.cardinality(ARRAY[{BIG},{BIG}])");
    // Act
    let result = fixture.execute(&sql).expect("qualified builtin count");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(2)]]);
}

#[test]
fn should_preserve_native_array_count_semantics() {
    // Arrange
    let fixture = started("cardinality-shape-semantics", &[]);
    // Act
    let rows = fixture.rows("SELECT cardinality(NULL::INT[]), cardinality(ARRAY[]::INT[]), cardinality(ARRAY[[1,2],[3,4]])");
    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Null, Value::Int64(0), Value::Int64(4)]]
    );
}

#[test]
fn should_reject_invalid_cardinality_signatures() {
    // Arrange
    let fixture = started("cardinality-invalid-signatures", &[]);
    // Act
    let errors = [
        "SELECT cardinality()",
        "SELECT cardinality(1)",
        "SELECT cardinality(ARRAY[1],ARRAY[2])",
    ]
    .map(|sql| fixture.execute(sql).expect_err("invalid native signature"));
    // Assert
    for error in errors {
        assert_ne!(error.sql_state(), "54000");
        assert!(error
            .to_string()
            .to_ascii_lowercase()
            .contains("cardinality"));
    }
}

#[test]
fn should_keep_schema_qualified_cardinality_on_its_udf_route() {
    // Arrange
    let fixture = started(
        "cardinality-udf-dispatch",
        &["CREATE FUNCTION public.cardinality(xs NUMERIC[]) RETURNS BIGINT AS \"88::bigint\""],
    );
    // Act
    let custom = fixture.rows("SELECT public.cardinality(ARRAY[1::numeric,2::numeric])");
    let builtin = fixture.rows("SELECT pg_catalog.cardinality(ARRAY[1::numeric,2::numeric])");
    let generic_limit = fixture.execute(&format!("SELECT public.cardinality(ARRAY[{BIG},{BIG}])"));
    // Assert
    assert_eq!(custom, vec![vec![Value::Int64(88)]]);
    assert_eq!(builtin, vec![vec![Value::Int64(2)]]);
    assert_eq!(
        generic_limit
            .expect_err("other functions retain generic workspace")
            .sql_state(),
        "54000"
    );
}

#[test]
fn should_preserve_numeric_overflow_errors_after_native_counting() {
    // Arrange
    let fixture = started("cardinality-numeric-overflow-controls", &[
        "CREATE TABLE v(id BIGINT PRIMARY KEY)",
        "CREATE TABLE e(id BIGINT PRIMARY KEY,a BIGINT,b BIGINT)",
        "INSERT INTO v VALUES(1),(2)", "INSERT INTO e VALUES(1,1,2)",
        "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
    ]);
    let vertex =
        |id| format!("cassie_graph.vertex_ref('g','n',ARRAY[cassie_graph.key_part({id}::bigint)])");
    let edge = "cassie_graph.edge_ref('g','l',ARRAY[cassie_graph.key_part(1::bigint)])";
    let path = format!("ROW(ARRAY[{},{},{}],ARRAY[{edge},{edge}],ARRAY[1::smallint,2::smallint])::cassie_graph.graph_path", vertex(1), vertex(2), vertex(1));
    // Act
    let scalar = fixture.execute(&format!("SELECT {BIG}+{BIG}"));
    let graph = fixture.execute(&format!(
        "SELECT * FROM cassie_graph.weighted_path_steps({path},ARRAY[{BIG},{BIG}])"
    ));
    let array = fixture.rows(&format!(
        "SELECT count(*) FROM unnest(ARRAY[{BIG},{BIG}]) AS n"
    ));
    // Assert
    assert_eq!(
        scalar.expect_err("scalar numeric overflow").sql_state(),
        "22003"
    );
    assert_eq!(
        graph.expect_err("graph numeric overflow").sql_state(),
        "22003"
    );
    assert_eq!(array, vec![vec![Value::Int64(2)]]);
}
