//! CTE identity is the parser-resolved identifier, including quoted contents.

use crate::query_correctness_fixture::started;
use cassie::app::CassieError;
use cassie::types::Value;

const CATALOG: &[&str] = &[
    "CREATE TABLE public.\"Base\"(x INT)",
    "INSERT INTO public.\"Base\" VALUES(2)",
    "CREATE TABLE public.base(x INT)",
    "INSERT INTO public.base VALUES(2)",
];

#[test]
fn should_resolve_a_quoted_local_cte_before_the_catalog() {
    // Arrange
    let fixture = started("cte-identity-direct", CATALOG);
    // Act
    let rows = fixture.rows("WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM \"Base\"");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(77)]]);
}

#[test]
fn should_inherit_quoted_cte_identity_in_a_nested_select() {
    // Arrange
    let fixture = started("cte-identity-nested", CATALOG);
    // Act
    let rows = fixture.rows(
        "WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM (SELECT x FROM \"Base\") AS local_rows",
    );
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(77)]]);
}

#[test]
fn should_fold_unquoted_cte_identifiers_once() {
    // Arrange
    let fixture = started("cte-identity-folding", CATALOG);
    // Act
    let rows = [
        "WITH BASE AS (SELECT 99 AS x) SELECT x FROM base",
        "WITH base AS (SELECT 99 AS x) SELECT x FROM BASE",
        "WITH \"base\" AS (SELECT 99 AS x) SELECT x FROM base",
        "WITH base AS (SELECT 99 AS x) SELECT x FROM \"base\"",
    ]
    .map(|sql| fixture.rows(sql));
    // Assert
    for values in rows {
        assert_eq!(values, vec![vec![Value::Int64(99)]]);
    }
}

#[test]
fn should_keep_qualified_catalog_references_outside_cte_lookup() {
    // Arrange
    let fixture = started("cte-identity-qualified", CATALOG);
    // Act
    let rows = fixture.rows("WITH \"Base\" AS (SELECT 77 AS x) SELECT x FROM public.\"Base\"");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_materialize_case_distinct_ctes_independently() {
    // Arrange
    let fixture = started("cte-identity-materialized-case", CATALOG);
    // Act
    let rows = fixture.rows("WITH base AS MATERIALIZED (SELECT 99 AS x), \"Base\" AS MATERIALIZED (SELECT 77 AS x) SELECT lo.x, hi.x FROM base AS lo JOIN \"Base\" AS hi ON TRUE");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(99), Value::Int64(77)]]);
}

#[test]
fn should_inline_case_distinct_ctes_independently() {
    // Arrange
    let fixture = started("cte-identity-inline-case", CATALOG);
    // Act
    let rows = fixture.rows("WITH base AS NOT MATERIALIZED (SELECT 99 AS x), \"Base\" AS NOT MATERIALIZED (SELECT 77 AS x) SELECT lo.x, hi.x FROM base AS lo JOIN \"Base\" AS hi ON TRUE");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(99), Value::Int64(77)]]);
}

#[test]
fn should_reject_duplicate_resolved_cte_identifiers() {
    // Arrange
    let fixture = started("cte-identity-duplicates", CATALOG);
    // Act
    let errors = [
        "WITH base AS (SELECT 1 AS x), BASE AS (SELECT 2 AS x) SELECT x FROM base",
        "WITH base AS (SELECT 1 AS x), \"base\" AS (SELECT 2 AS x) SELECT x FROM base",
        "WITH \"Base\" AS (SELECT 1 AS x), \"Base\" AS (SELECT 2 AS x) SELECT x FROM \"Base\"",
    ]
    .map(|sql| fixture.execute(sql).expect_err("duplicate identity"));
    // Assert
    for error in errors {
        assert!(
            matches!(error, CassieError::Planner(message) if message.contains("duplicate CTE name"))
        );
    }
}

#[test]
fn should_preserve_cte_identifier_contents() {
    // Arrange
    let fixture = started(
        "cte-identity-contents",
        &[
            "CREATE TABLE public.\"part.one\"(x INT)",
            "INSERT INTO public.\"part.one\" VALUES(2)",
            "CREATE TABLE public.\"say\"\"Hi\"(x INT)",
            "INSERT INTO public.\"say\"\"Hi\" VALUES(2)",
        ],
    );
    // Act
    let dotted = fixture.rows("WITH \"part.one\" AS (SELECT 77 AS x) SELECT x FROM \"part.one\"");
    let escaped =
        fixture.rows("WITH \"say\"\"Hi\" AS (SELECT 88 AS x) SELECT x FROM \"say\"\"Hi\"");
    // Assert
    assert_eq!(dotted, vec![vec![Value::Int64(77)]]);
    assert_eq!(escaped, vec![vec![Value::Int64(88)]]);
}

#[test]
fn should_preserve_nonrecursive_body_visibility() {
    // Arrange
    let fixture = started("cte-identity-body-scope", CATALOG);
    // Act
    let own_body = fixture.rows("WITH \"Base\" AS (SELECT x FROM \"Base\") SELECT x FROM \"Base\"");
    let prior = fixture.rows(
        "WITH \"Base\" AS (SELECT 77 AS x), later AS (SELECT x FROM \"Base\") SELECT x FROM later",
    );
    // Assert
    assert_eq!(own_body, vec![vec![Value::Int64(2)]]);
    assert_eq!(prior, vec![vec![Value::Int64(77)]]);
}

#[test]
fn should_resolve_a_quoted_recursive_self_reference() {
    // Arrange
    let fixture = started(
        "cte-identity-recursive",
        &[
            "CREATE TABLE public.\"Walk\"(n INT)",
            "INSERT INTO public.\"Walk\" VALUES(2)",
        ],
    );
    assert_eq!(
        fixture.rows("WITH RECURSIVE walk(n) AS (SELECT 1::INT UNION ALL SELECT CAST(n+1 AS INT) FROM walk WHERE n<2) SELECT n FROM walk ORDER BY n"),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    // Act
    let rows = fixture.rows("WITH RECURSIVE \"Walk\"(n) AS (SELECT 1::INT UNION ALL SELECT CAST(n+1 AS INT) FROM \"Walk\" WHERE n<2) SELECT n FROM \"Walk\" ORDER BY n");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_preserve_quoted_cte_wildcard_descriptors() {
    // Arrange
    let fixture = started(
        "cte-identity-descriptor",
        &[
            "CREATE TABLE public.\"Choice\"(renamed TEXT)",
            "INSERT INTO public.\"Choice\" VALUES('catalog')",
        ],
    );
    // Act
    let result = fixture
        .execute(
            "WITH \"Choice\"(renamed) AS MATERIALIZED (SELECT 77::INT) SELECT * FROM \"Choice\"",
        )
        .expect("typed local wildcard");
    // Assert
    assert_eq!(result.rows(), &[vec![Value::Int64(77)]]);
    assert_eq!(result.columns()[0].name, "renamed");
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_restore_outer_cte_identity_after_a_nested_shadow() {
    // Arrange
    let fixture = started("cte-identity-nested-shadow", CATALOG);
    // Act
    let rows = fixture.rows("WITH \"Base\" AS (SELECT 77 AS x) SELECT outer_rows.x, inner_rows.x FROM \"Base\" AS outer_rows JOIN (WITH \"Base\" AS (SELECT 88 AS x) SELECT x FROM \"Base\") AS inner_rows ON TRUE");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(77), Value::Int64(88)]]);
}

#[test]
fn should_resolve_implicit_qualifiers_of_case_distinct_ctes() {
    // Arrange
    let fixture = started("cte-implicit-case-qualifiers", CATALOG);
    let queries = [
        r#"WITH "Walk"(n) AS (SELECT 4), "walk"(n) AS (SELECT 9)
           SELECT "Walk".n, "walk".n FROM "Walk" CROSS JOIN "walk""#,
        r#"WITH "A.B"(n) AS (SELECT 4::INT), "a.b"(n) AS (SELECT 9::INT)
           SELECT "A.B".n,"a.b".n FROM "A.B" CROSS JOIN "a.b""#,
        r#"WITH "A.B"(n) AS (SELECT 4::INT), "a.b"(n) AS (SELECT 9::INT)
           SELECT u.n,"a.b".n FROM "A.B" u CROSS JOIN "a.b""#,
        r#"WITH "walk"(n) AS (SELECT 9), "Walk"(n) AS (SELECT 4)
           SELECT "Walk".n, walk.n FROM walk CROSS JOIN "Walk""#,
        r#"WITH "Walk"(n) AS (SELECT 4), walk(n) AS (SELECT 9)
           SELECT a.n,b.n FROM "Walk" a CROSS JOIN walk b"#,
    ];

    // Act
    let results = queries.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in queries.into_iter().zip(results) {
        assert_eq!(
            result
                .unwrap_or_else(|error| panic!("{sql}: {error}"))
                .rows(),
            [vec![Value::Int64(4), Value::Int64(9)]],
            "{sql}"
        );
    }
}
