use cassie::sql::ast::QueryStatement;
use cassie::sql::parse_statement;

const QUERY_SEPARATOR_PAIRS: &[(&str, &str)] = &[
        ("SELECT * FROM (SELECT a FROM t) AS x", "SELECT * FROM (SELECT a FROM t)/**/AS/**/x"),
        ("SELECT l.a,x.a FROM l JOIN LATERAL (SELECT a FROM t) AS x ON true", "SELECT l.a,x.a FROM l JOIN LATERAL\n(SELECT a FROM t) AS\nx ON true"),
        ("SELECT * FROM (SELECT a FROM t) AS x", "SELECT * FROM (SELECT a /* ) ' */ FROM t) AS x"),

            ("SELECT CASE WHEN a=1 THEN 2 ELSE 3 END FROM t", "SELECT CASE/*END*/WHEN a=1 THEN/*WHEN*/2 ELSE 3 END FROM t"),
            ("SELECT CAST(a AS INT) FROM t", "SELECT CAST/*separator*/(a AS\nINT) FROM t"),

            ("SELECT DISTINCT ON(a) a FROM t ORDER BY a DESC NULLS FIRST", "SELECT DISTINCT\nON(a) a FROM t ORDER\nBY a\tDESC NULLS\nFIRST"),
            ("SELECT first_value(a) OVER (PARTITION BY b ORDER BY a ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t", "SELECT first_value(a) OVER (PARTITION\nBY b ORDER\tBY a ROWS BETWEEN\nUNBOUNDED\tPRECEDING AND CURRENT\nROW) FROM t"),

            (
                "INSERT INTO t(a) VALUES(1) ON CONFLICT(a) DO UPDATE SET a=excluded.a",
                "INSERT\nINTO t(a) VALUES(1) ON\nCONFLICT(a) DO UPDATE\nSET a=excluded.a",
            ),
            ("DELETE FROM t WHERE a=1", "DELETE\nFROM t WHERE a=1"),
            (
                "SELECT '\\' AS a FROM t ORDER BY a",
                "SELECT '\\' AS a FROM t ORDER\nBY a",
            ),
            (
                "SELECT a FROM t",
                "SELECT a FROM t; -- trailing statement comment",
            ),
            (
                "SELECT a FROM t",
                "/*leading SELECT*/ SELECT a FROM/*source*/t -- trailing ORDER BY",
            ),
            (
                "SELECT a,b FROM t",
                "SELECT /*comma ,*/a,/*ignored ,*/b FROM t",
            ),
            (
                "SELECT a FROM t WHERE a BETWEEN 1 AND 2 AND b BETWEEN 3 AND 4",
                "SELECT a FROM t WHERE a BETWEEN 1\nAND 2\tAND b BETWEEN 3/*gap*/AND 4",
            ),
            ("SELECT a FROM t ORDER BY a", "SELECT\na FROM t ORDER\nBY a"),
            (
                "SELECT a FROM t GROUP BY a",
                "SELECT a FROM t GROUP\t  BY a",
            ),
            (
                "SELECT a FROM t WHERE a = 1 AND b = 2",
                "SELECT a FROM t WHERE a = 1\nAND\tb = 2",
            ),
            (
                "SELECT a FROM t WHERE a BETWEEN 1 AND 2",
                "SELECT a FROM t WHERE a BETWEEN 1\nAND 2",
            ),
            (
                "SELECT a FROM t WHERE a IS NOT NULL",
                "SELECT a FROM t WHERE a IS\tNOT\nNULL",
            ),
            (
                "SELECT a FROM t WHERE a NOT IN (1,2)",
                "SELECT a FROM t WHERE a NOT\n IN (1,2)",
            ),
            (
                "SELECT a FROM t ORDER BY a",
                "SELECT/*SELECT WHERE*/a FROM t ORDER/*nested /*inner*/ comment*/BY a",
            ),
            (
                "SELECT l.a FROM l LEFT JOIN r ON l.a = r.a",
                "SELECT l.a FROM l LEFT\nJOIN r\nON\tl.a = r.a",
            ),
            (
                "SELECT a FROM t UNION ALL SELECT a FROM t",
                "SELECT a FROM t UNION/*separator*/ALL SELECT a FROM t",
            ),
            (
                "SELECT a FROM t WHERE a = 1 OR b = 2",
                "SELECT a FROM t WHERE a = 1-- OR false\nOR b = 2",
            ),
        ];

#[test]
fn should_preserve_query_semantics_across_sql_separators() {
    // Arrange
    for &(canonical, formatted) in QUERY_SEPARATOR_PAIRS {
        // Act
        let expected = parse_statement(canonical).expect("canonical query");
        let actual = parse_statement(formatted).expect(formatted);
        // Assert
        assert_eq!(
            format!("{:?}", actual.statement),
            format!("{:?}", expected.statement),
            "{formatted}"
        );
        assert_eq!(actual.raw_sql, formatted);
    }
}

#[test]
fn should_parse_explicit_join_kind_spellings() {
    // Arrange
    for (explicit, canonical) in [
        ("INNER JOIN", "JOIN"),
        ("LEFT OUTER JOIN", "LEFT JOIN"),
        ("RIGHT OUTER JOIN", "RIGHT JOIN"),
        ("FULL OUTER JOIN", "FULL JOIN"),
    ] {
        // Act
        let expected =
            parse_statement(&format!("SELECT l.a FROM l {canonical} r ON l.a = r.a")).unwrap();
        let actual = parse_statement(&format!("SELECT l.a FROM l {explicit} r ON l.a = r.a"))
            .expect(explicit);
        // Assert
        assert_eq!(
            format!("{:?}", actual.statement),
            format!("{:?}", expected.statement)
        );
    }
}

#[test]
fn should_preserve_quoted_sql_bytes_while_matching_separators() {
    // Arrange
    let sql = "SELECT 'café ORDER\nBY -- /* */' AS \"GROUP BY\" FROM t ORDER\nBY \"GROUP BY\"";
    let routine = "CREATE FUNCTION preserved_body() RETURNS TEXT AS \"'ORDER\nBY -- /* */'\"";
    let view = "CREATE VIEW v AS SELECT 'ORDER\nBY' AS a FROM t ORDER\nBY a";
    // Act
    let parsed = parse_statement(sql).expect("quoted query");
    let routine = parse_statement(routine).expect("quoted routine body");
    let stored = parse_statement(view).expect("stored view");
    // Assert
    assert_eq!(parsed.raw_sql, sql);
    assert!(routine.raw_sql.contains("ORDER\nBY -- /* */"));
    let QueryStatement::CreateView(statement) = stored.statement else {
        panic!("view expected");
    };
    assert_eq!(
        statement.query,
        "SELECT 'ORDER\nBY' AS a FROM t ORDER\nBY a"
    );
}
#[test]
fn should_preserve_nested_query_sql_text() {
    // Arrange
    let nested = "SELECT a /*preserved query comment*/ FROM t";
    // Act
    let parsed =
        parse_statement(&format!("SELECT 1 WHERE EXISTS({nested})")).expect("nested query");
    // Assert
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT expected");
    };
    let Some(cassie::sql::ast::Expr::Exists(query)) = select.filter else {
        panic!("EXISTS expected");
    };
    assert_eq!(query.raw_sql, nested);
}
#[test]
fn should_parse_supported_query_tokens_with_each_separator_form() {
    // Arrange
    let queries=[
            "SELECT a FROM t WHERE a BETWEEN 1 AND 2 AND a IS NOT NULL ORDER BY a DESC NULLS LAST LIMIT 1 OFFSET 0",
            "SELECT l.a FROM l LEFT OUTER JOIN r ON l.a = r.a",
            "SELECT l.a,x.a FROM l JOIN LATERAL (SELECT a FROM t) AS x ON true",
            "SELECT CASE WHEN a = 1 THEN 2 ELSE 3 END FROM t",
            "SELECT CAST(a AS INT) FROM t",
            "SELECT first_value(a) OVER (PARTITION BY b ORDER BY a ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t",
            "INSERT INTO t(a) VALUES(1) ON CONFLICT(a) DO UPDATE SET a=excluded.a",
            "UPDATE t SET a=1 WHERE a=2",
            "DELETE FROM t WHERE a=1",
        ];
    for canonical in queries {
        let expected = parse_statement(canonical).expect("canonical query");
        for separator in [
            "  ",
            "\n",
            "\t",
            "/**/",
            "/*( SELECT JOIN AND )*/",
            "/* ) ' , = AND */",
            "\u{2003}\t/* JOIN */\n",
        ] {
            let formatted = canonical.replace(' ', separator);
            // Act
            let actual = parse_statement(&formatted).expect(&formatted);
            // Assert
            assert_eq!(
                format!("{:?}", actual.statement),
                format!("{:?}", expected.statement),
                "{formatted}"
            );
            assert_eq!(actual.raw_sql, formatted);
        }
    }
}
#[test]
fn should_parse_cte_keyword_separators_without_losing_recursive_scope() {
    // Arrange
    let sql="WITH/* ) ' */RECURSIVE/*gap*/seq/*header*/(n/*alias*/)/*head*/AS/*body*/(SELECT 1/* ) ' */ UNION/*gap*/ALL SELECT n+1 FROM seq WHERE n<2) SELECT\nn FROM seq";
    // Act
    let parsed = parse_statement(sql).expect("recursive CTE separators");
    // Assert
    assert_eq!(parsed.raw_sql, sql);
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT expected");
    };
    assert!(select.recursive);
    assert_eq!(select.ctes[0].name, "seq");
    assert_eq!(select.ctes[0].aliases, ["n"]);
}
