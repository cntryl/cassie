// Integration suite: derived tables and CTEs as FROM sources — their column
// names, column types and predicate typing must match the base table they
// read from.

#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod derived_result_metadata {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    const TEXT_OID: i64 = 25;
    const INT4_OID: i64 = 23;
    const FLOAT8_OID: i64 = 701;
    const BOOL_OID: i64 = 16;

    fn typed_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE t (name TEXT, n INT, f FLOAT, flag BOOLEAN)",
                "CREATE TABLE other (a INT, c TEXT)",
                "INSERT INTO t (name, n, f, flag) VALUES ('x', 7, 1.5, true)",
                "INSERT INTO other (a, c) VALUES (9, 'other-nine')",
            ],
        )
    }

    fn column_oids(fixture: &SqlFixture, sql: &str) -> Vec<(String, i64)> {
        let Ok(result) = fixture.execute(sql) else {
            panic!("query failed: {sql}");
        };
        result
            .columns
            .into_iter()
            .map(|column| (column.name, column.type_oid))
            .collect()
    }

    #[test]
    fn should_describe_derived_table_columns_with_their_real_types() {
        // Arrange
        let fixture = typed_fixture("derived_types");
        let expected = vec![
            ("n".to_string(), INT4_OID),
            ("f".to_string(), FLOAT8_OID),
            ("flag".to_string(), BOOL_OID),
        ];

        // Act
        let derived = column_oids(&fixture, "SELECT n, f, flag FROM (SELECT * FROM t) d");
        let described = fixture
            .cassie
            .describe_sql("SELECT n, f, flag FROM (SELECT * FROM t) d");

        // Assert
        assert_eq!(derived, expected);
        let Ok(described) = described else {
            panic!("describe failed");
        };
        let described: Vec<(String, i64)> = described
            .into_iter()
            .map(|column| (column.name, column.type_oid))
            .collect();
        assert_eq!(described, expected);
    }

    #[test]
    fn should_not_adopt_a_same_named_table_schema_for_an_alias() {
        // Arrange
        let fixture = typed_fixture("derived_alias_collision");
        let expected = vec![("a".to_string(), INT4_OID), ("c".to_string(), TEXT_OID)];

        // Act
        let derived = column_oids(&fixture, "SELECT * FROM (SELECT a, c FROM other) t");
        let cte = column_oids(
            &fixture,
            "WITH t AS (SELECT a, c FROM other) SELECT * FROM t",
        );
        let rows = fixture.rows("SELECT * FROM (SELECT a, c FROM other) t");

        // Assert
        assert_eq!(derived, expected);
        assert_eq!(cte, expected);
        assert_eq!(
            rows,
            vec![vec![
                Value::Int64(9),
                Value::String("other-nine".to_string())
            ]]
        );
    }
}

mod derived_predicate_typing {
    use cassie::types::Value;

    use crate::support_sql_fixture::{sql_fixture, SqlFixture};

    const UPPER_UUID: &str = "01234567-89AB-CDEF-0123-456789ABCDEF";

    fn uuid_fixture(label: &str) -> SqlFixture {
        sql_fixture(
            label,
            &[
                "CREATE TABLE t (uid UUID, txt TEXT, blob BYTEA, n INT)",
                "INSERT INTO t (uid, txt, blob, n) VALUES \
                 ('01234567-89ab-cdef-0123-456789abcdef', \
                  '01234567-89AB-CDEF-0123-456789ABCDEF', '\\xdeadbeef', 7)",
            ],
        )
    }

    #[test]
    fn should_type_renamed_derived_columns_by_their_own_source() {
        // Arrange
        let fixture = uuid_fixture("derived_renames");

        // Act
        let renamed = fixture.rows(&format!(
            "SELECT n FROM (SELECT txt AS uid, n FROM t) d WHERE uid = '{UPPER_UUID}'"
        ));
        let plain_text =
            fixture.execute("SELECT n FROM (SELECT txt AS uid, n FROM t) d WHERE uid = 'hello'");

        // Assert
        assert_eq!(renamed, vec![vec![Value::Int64(7)]]);
        assert!(plain_text.is_ok(), "a TEXT column accepts any literal");
    }

    #[test]
    fn should_canonicalize_typed_literals_over_a_cte() {
        // Arrange
        let fixture = uuid_fixture("cte_typed_literals");

        // Act
        let uuid = fixture.rows(&format!(
            "WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE uid = '{UPPER_UUID}'"
        ));
        let bytea =
            fixture.rows("WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE blob = '\\xDEADBEEF'");
        let invalid =
            fixture.execute("WITH c AS (SELECT * FROM t) SELECT n FROM c WHERE uid = 'nope'");
        let nested = fixture.rows(&format!(
            "WITH c AS (SELECT * FROM t) SELECT n FROM (SELECT * FROM c) d WHERE uid = '{UPPER_UUID}'"
        ));

        // Assert
        assert_eq!(uuid, vec![vec![Value::Int64(7)]]);
        assert_eq!(nested, vec![vec![Value::Int64(7)]]);
        assert_eq!(bytea, vec![vec![Value::Int64(7)]]);
        assert!(
            invalid.is_err(),
            "an invalid UUID literal must be rejected through a CTE"
        );
    }

    #[test]
    fn should_validate_operand_families_over_a_cte() {
        // Arrange
        let fixture = sql_fixture(
            "cte_operand_families",
            &[
                "CREATE TABLE d (k INT, flag BOOLEAN, price FLOAT)",
                "INSERT INTO d (k, flag, price) VALUES (1, true, 19.99)",
            ],
        );

        // Act
        let base = fixture.execute("SELECT k FROM d WHERE flag = 1").is_err();
        let cte = fixture
            .execute("WITH c AS (SELECT * FROM d) SELECT k FROM c WHERE flag = 1")
            .is_err();

        let base_literal = fixture
            .execute("SELECT k FROM d WHERE k = '1'")
            .map(|result| result.rows)
            .ok();
        let cte_literal = fixture
            .execute("WITH c AS (SELECT * FROM d) SELECT k FROM c WHERE k = '1'")
            .map(|result| result.rows)
            .ok();

        // Assert
        assert_eq!(cte_literal, base_literal);
        assert!(base, "the base table rejects boolean = integer");
        assert!(cte, "the CTE spelling must reject it too");
    }
}

mod cte_alias_lists {
    use cassie::types::Value;

    use crate::support_sql_fixture::sql_fixture;

    #[test]
    fn should_apply_cte_alias_lists_over_wildcard_bodies() {
        // Arrange
        let fixture = sql_fixture(
            "cte_alias_wildcard",
            &[
                "CREATE TABLE two (id INT, x INT, y INT)",
                "INSERT INTO two (id, x, y) VALUES (1, 2, 3)",
            ],
        );

        // Act
        let full = fixture.rows("WITH c(p, q, r) AS (SELECT * FROM two) SELECT r, p FROM c");
        let partial = fixture.execute("WITH c(p) AS (SELECT * FROM two) SELECT p, x, y FROM c");
        let too_many = fixture.execute("WITH c(p, q, r, s) AS (SELECT * FROM two) SELECT p FROM c");

        // Assert
        assert_eq!(full, vec![vec![Value::Int64(3), Value::Int64(1)]]);
        let Ok(partial) = partial else {
            panic!("a shorter alias list renames only the leading columns");
        };
        assert_eq!(
            partial.rows,
            vec![vec![Value::Int64(1), Value::Int64(2), Value::Int64(3)]]
        );
        assert!(too_many.is_err(), "more aliases than columns is an error");
    }
}

mod derived_fulltext_options {
    use cassie::types::Value;

    use crate::support_sql_fixture::sql_fixture;

    #[test]
    fn should_apply_fulltext_index_options_through_derived_sources() {
        // Arrange
        let fixture = sql_fixture(
            "derived_fulltext",
            &[
                "CREATE TABLE docs (title TEXT, body TEXT)",
                "CREATE INDEX idx_docs_body ON docs USING fulltext (body) \
                 WITH (stop_words = none, boost = 3.0)",
                "INSERT INTO docs (title, body) VALUES ('a', 'the quick brown fox')",
            ],
        );
        let score = |sql: &str| match fixture.rows(sql).first().and_then(|row| row.first()) {
            Some(Value::Float64(value)) => *value,
            other => panic!("expected a score, got {other:?}"),
        };

        // Act
        let base = fixture.rows("SELECT title FROM docs WHERE search(body, 'the')");
        let derived =
            fixture.rows("SELECT title FROM (SELECT * FROM docs) d WHERE search(body, 'the')");
        let cte = fixture
            .rows("WITH c AS (SELECT * FROM docs) SELECT title FROM c WHERE search(body, 'the')");
        let base_score = score("SELECT search_score(body, 'quick') FROM docs");
        let derived_score = score("SELECT search_score(body, 'quick') FROM (SELECT * FROM docs) d");

        // Assert
        assert_eq!(base.len(), 1);
        assert_eq!(derived, base);
        assert_eq!(cte, base);
        assert!(
            (base_score - derived_score).abs() < 1e-9,
            "scores must match"
        );
    }
}
