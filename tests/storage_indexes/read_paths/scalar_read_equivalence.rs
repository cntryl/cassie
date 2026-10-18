use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

fn ids(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    statement: &str,
) -> Vec<Value> {
    let mut values = sql(cassie, session, statement)
        .rows()
        .iter()
        .map(|row| row.first().expect("identity").clone())
        .collect::<Vec<_>>();
    values.sort_by_key(|value| format!("{value:?}"));
    values
}

#[test]
fn should_preserve_constant_expression_bounds_after_scalar_index_creation() {
    // Arrange
    with_fixture("constant_index_bounds", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE constant_bounds (a INT, b TEXT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO constant_bounds VALUES (5,'five'),(6,'six')",
        );
        let predicates = [
            "a = CAST('5' AS INT)",
            "a = 5::INT",
            "a = abs(5)",
            "a = 2 + 3",
            "a = CASE WHEN true THEN 5 ELSE 6 END",
            "a >= CAST('5' AS INT)",
            "CAST('5' AS INT) = a",
            "a BETWEEN CAST('4' AS INT) AND 6",
            "b = lower('FIVE')",
        ];
        let baseline = predicates
            .iter()
            .map(|predicate| {
                ids(
                    cassie,
                    session,
                    &format!("SELECT a FROM constant_bounds WHERE {predicate}"),
                )
            })
            .collect::<Vec<_>>();
        sql(
            cassie,
            session,
            "CREATE INDEX constant_a ON constant_bounds(a)",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX constant_b ON constant_bounds(b)",
        );
        // Act
        for (predicate, expected) in predicates.iter().zip(baseline) {
            let indexed = ids(
                cassie,
                session,
                &format!("SELECT a FROM constant_bounds WHERE {predicate}"),
            );
            // Assert
            assert_eq!(indexed, expected, "{predicate}");
        }
    });
}

#[test]
fn should_preserve_constant_bounds_across_scalar_index_shapes() {
    // Arrange
    with_fixture("constant_index_siblings", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE expression_bounds (a TEXT, b TEXT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO expression_bounds VALUES ('X','match'),('Y','other')",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX expression_lower ON expression_bounds(lower(a))",
        );
        sql(
            cassie,
            session,
            "CREATE TABLE composite_bounds (a INT NOT NULL,b TEXT NOT NULL,c TEXT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO composite_bounds VALUES (1,'x','match'),(1,'y','other')",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX composite_ab ON composite_bounds(a,b)",
        );
        sql(cassie, session, "CREATE VIEW bounds_view AS SELECT c FROM composite_bounds WHERE a = CAST('1' AS INT) AND b = lower('X')");
        sql(
            cassie,
            session,
            "CREATE TABLE primary_bounds (a INT PRIMARY KEY,b TEXT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO primary_bounds VALUES (5,'match'),(6,'other')",
        );
        // Act
        let results = [
            sql(
                cassie,
                session,
                "SELECT b FROM expression_bounds WHERE lower(a) = lower('X')",
            ),
            sql(
                cassie,
                session,
                "SELECT b FROM expression_bounds WHERE lower(a) = CAST('x' AS TEXT)",
            ),
            sql(
                cassie,
                session,
                "SELECT c FROM composite_bounds WHERE a = CAST('1' AS INT) AND b = 'x'",
            ),
            sql(cassie, session, "SELECT c FROM bounds_view"),
            cassie
                .execute_sql(
                    session,
                    "SELECT b FROM primary_bounds WHERE a = $1::INT",
                    vec![Value::Int64(5)],
                )
                .expect("parameter cast"),
        ];
        // Assert
        for result in results {
            assert_eq!(result.rows(), vec![vec![Value::String("match".into())]]);
        }
    });
}

#[test]
fn should_preserve_exact_integer_bounds_on_float_indexes() {
    // Arrange
    with_fixture("float_exact_index_bounds", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE float_bounds (id TEXT,x FLOAT)",
        );
        sql(cassie, session, "INSERT INTO float_bounds VALUES ('positive',9007199254740992.0),('negative',-9007199254740992.0)");
        let predicates = [
            "x = 9007199254740993",
            "x < 9007199254740993",
            "x > 9007199254740993",
            "x = -9007199254740993",
            "x <= -9007199254740993",
            "x >= -9007199254740993",
            "x = 9007199254740992",
        ];
        let baseline = predicates
            .iter()
            .map(|predicate| {
                ids(
                    cassie,
                    session,
                    &format!("SELECT id FROM float_bounds WHERE {predicate}"),
                )
            })
            .collect::<Vec<_>>();
        sql(
            cassie,
            session,
            "CREATE INDEX exact_float_x ON float_bounds(x)",
        );
        // Act
        for (predicate, expected) in predicates.iter().zip(baseline) {
            let indexed = ids(
                cassie,
                session,
                &format!("SELECT id FROM float_bounds WHERE {predicate}"),
            );
            // Assert
            assert_eq!(indexed, expected, "{predicate}");
        }
    });
}

#[test]
fn should_preserve_timestamp_varchar_index_comparisons() {
    // Arrange
    with_fixture("timestamp_varchar_index_equivalence", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE timestamp_varchar (id INT,code VARCHAR(40) NOT NULL)",
        );
        sql(cassie, session, "INSERT INTO timestamp_varchar VALUES (1,'2024-01-01T00:00:00Z'),(2,'2024-01-01T00:00:00.000000Z')");
        let query = "SELECT id FROM timestamp_varchar WHERE code = '2024-01-01T00:00:00Z'";
        let baseline = ids(cassie, session, query);
        sql(
            cassie,
            session,
            "CREATE INDEX timestamp_varchar_idx ON timestamp_varchar(code)",
        );
        // Act
        let indexed = ids(cassie, session, query);
        // Assert
        assert_eq!(indexed, baseline);
    });
}

#[test]
fn should_preserve_timestamp_text_expression_index_ordering() {
    // Arrange
    with_fixture(
        "timestamp_expression_order_equivalence",
        |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE timestamp_order_expression (id INT,code TEXT NOT NULL)",
            );
            sql(cassie, session, "INSERT INTO timestamp_order_expression VALUES (1,'2024-01-01T00:00:00Z'),(2,'2024-01-01T00:00:00.000001Z')");
            let query =
                "SELECT id FROM timestamp_order_expression ORDER BY upper(code) ASC LIMIT 1";
            let baseline = sql(cassie, session, query);
            sql(cassie, session, "CREATE INDEX timestamp_order_expression_idx ON timestamp_order_expression(upper(code))");
            // Act
            let indexed = sql(cassie, session, query);
            // Assert
            assert_eq!(indexed.rows(), baseline.rows());
        },
    );
}

#[test]
fn should_preserve_timestamp_text_expression_index_comparisons() {
    // Arrange
    with_fixture(
        "timestamp_text_expression_equivalence",
        |cassie, session| {
            sql(
                cassie,
                session,
                "CREATE TABLE timestamp_expression (id INT,code TEXT)",
            );
            sql(cassie, session, "INSERT INTO timestamp_expression VALUES (1,'2024-01-01T00:00:00Z'),(2,'2024-01-01T00:00:00.000000Z'),(3,'ordinary')");
            let query =
                "SELECT id FROM timestamp_expression WHERE upper(code) = '2024-01-01T00:00:00Z'";
            let baseline = ids(cassie, session, query);
            sql(
                cassie,
                session,
                "CREATE INDEX timestamp_expression_idx ON timestamp_expression(upper(code))",
            );
            // Act
            let indexed = ids(cassie, session, query);
            // Assert
            assert_eq!(indexed, baseline);
        },
    );
}

#[test]
fn should_preserve_canonical_timestamp_text_comparisons_after_index_creation() {
    // Arrange
    with_fixture("timestamp_text_index_equivalence", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE timestamp_text (id INT,code TEXT NOT NULL)",
        );
        sql(cassie, session, "INSERT INTO timestamp_text VALUES (1,'2024-01-01T00:00:00Z'),(2,'2024-01-01T00:00:00.000000Z'),(3,'2024-01-01T00:00:00.000001Z'),(4,'ordinary')");
        let predicates = [
            "code = '2024-01-01T00:00:00Z'",
            "code = '2024-01-01T00:00:00.000Z'",
            "code > '2024-01-01T00:00:00Z'",
            "code <= '2024-01-01T00:00:00.000Z'",
            "code = 'ordinary'",
        ];
        let baseline = predicates
            .iter()
            .map(|predicate| {
                ids(
                    cassie,
                    session,
                    &format!("SELECT id FROM timestamp_text WHERE {predicate}"),
                )
            })
            .collect::<Vec<_>>();
        let ordered_baseline = sql(
            cassie,
            session,
            "SELECT id FROM timestamp_text ORDER BY code LIMIT 4",
        );
        sql(
            cassie,
            session,
            "CREATE INDEX timestamp_text_code ON timestamp_text(code)",
        );
        // Act
        for (predicate, expected) in predicates.iter().zip(baseline) {
            let indexed = ids(
                cassie,
                session,
                &format!("SELECT id FROM timestamp_text WHERE {predicate}"),
            );
            // Assert
            assert_eq!(indexed, expected, "{predicate}");
        }
        let ordered = sql(
            cassie,
            session,
            "SELECT id FROM timestamp_text ORDER BY code LIMIT 4",
        );
        assert_eq!(ordered.rows(), ordered_baseline.rows());
    });
}
