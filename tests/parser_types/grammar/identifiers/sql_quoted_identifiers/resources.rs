#[test]
fn should_resolve_case_distinct_delimited_projection_aliases_exactly() {
    with_users_table("quoted_ident_case_distinct_aliases", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE alias_case_distinct (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO alias_case_distinct (\"a\", \"A\") VALUES (2, 1), (1, 2)",
        );

        // Act
        let upper_alias = run(
            cassie,
            session,
            "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY \"X\"",
        );
        let upper_ordinal = run(
            cassie,
            session,
            "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY 2",
        );
        let grouped_upper_alias = run(
            cassie,
            session,
            "SELECT \"a\" AS \"x\", \"A\" AS \"X\", count(*) \
                 FROM alias_case_distinct GROUP BY \"a\", \"A\" ORDER BY \"X\"",
        );
        let lower_alias = run(
            cassie,
            session,
            "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY \"x\"",
        );

        // Assert
        let ordered_by_upper = vec![
            vec![Value::Int64(2), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2)],
        ];
        assert_eq!(upper_alias.rows(), ordered_by_upper);
        assert_eq!(upper_ordinal.rows(), ordered_by_upper);
        assert_eq!(
            grouped_upper_alias.rows(),
            vec![
                vec![Value::Int64(2), Value::Int64(1), Value::Int64(1)],
                vec![Value::Int64(1), Value::Int64(2), Value::Int64(1)],
            ]
        );
        assert_eq!(
            lower_alias.rows(),
            vec![
                vec![Value::Int64(1), Value::Int64(2)],
                vec![Value::Int64(2), Value::Int64(1)],
            ]
        );
        assert_eq!(
            upper_alias
                .columns()
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            ["x", "X"]
        );
    });
}

#[test]
fn should_preserve_case_distinct_delimited_cte_aliases() {
    with_users_table(
        "quoted_ident_case_distinct_cte_aliases",
        |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE cte_alias_case_distinct (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO cte_alias_case_distinct (\"a\", \"A\") VALUES (2, 1), (1, 2)",
            );

            // Act
            let selected = run(
                cassie,
                session,
                "WITH named (\"x\", \"X\") AS \
                 (SELECT \"a\", \"A\" FROM cte_alias_case_distinct) \
                 SELECT \"x\", \"X\" FROM named ORDER BY \"X\"",
            );

            // Assert
            assert_eq!(
                selected.rows(),
                vec![
                    vec![Value::Int64(2), Value::Int64(1)],
                    vec![Value::Int64(1), Value::Int64(2)],
                ]
            );
            assert_eq!(
                selected
                    .columns()
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                ["x", "X"]
            );
        },
    );
}

#[test]
fn should_fold_undelimited_column_aliases() {
    // Arrange
    let parsed = parse_statement("SELECT 1 AS Total").expect("parse projection alias");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT statement");
    };

    // Act
    let aliases = select
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Expr { alias, .. } => alias.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(aliases, vec![Some("total")]);
}

#[test]
fn should_preserve_delimited_column_alias_spelling() {
    // Arrange
    let parsed = parse_statement("SELECT 1 AS \"Exact\"").expect("parse projection alias");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("expected SELECT statement");
    };

    // Act
    let aliases = select
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Expr { alias, .. } => alias.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(aliases, vec![Some("Exact")]);
}

#[test]
fn should_match_delimited_column_identifiers_exactly() {
    with_users_table("quoted_ident_exact_case", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE TABLE exact_case (\"Email\" TEXT)");
        run(
            cassie,
            session,
            "INSERT INTO exact_case (\"Email\") VALUES ('ada')",
        );

        // Act
        let exact = run(cassie, session, "SELECT \"Email\" FROM exact_case");
        let mismatched = cassie.execute_sql(session, "SELECT \"EMAIL\" FROM exact_case", vec![]);
        let undelimited = cassie.execute_sql(session, "SELECT email FROM exact_case", vec![]);

        // Assert
        assert_eq!(exact.rows(), string_rows(&["ada"]));
        assert!(mismatched.is_err(), "quoted spelling must match exactly");
        assert!(
            undelimited.is_err(),
            "email must not resolve to stored Email"
        );
    });
}

#[test]
fn should_fold_undelimited_column_identifiers_to_lowercase() {
    with_users_table("unquoted_ident_lowercase", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE TABLE folded_case (Email TEXT)");
        run(
            cassie,
            session,
            "INSERT INTO folded_case (EMAIL) VALUES ('ada')",
        );

        // Act
        let unquoted = run(cassie, session, "SELECT email FROM folded_case");
        let quoted_original_case =
            cassie.execute_sql(session, "SELECT \"Email\" FROM folded_case", vec![]);

        // Assert
        assert_eq!(unquoted.rows(), string_rows(&["ada"]));
        assert!(
            quoted_original_case.is_err(),
            "unquoted declarations fold to lowercase"
        );
    });
}

#[test]
fn should_keep_delimited_columns_that_differ_only_by_case() {
    with_users_table("quoted_ident_case_distinct", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE case_distinct (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO case_distinct (\"a\", \"A\") VALUES (1, 2)",
        );

        // Act
        let selected = run(cassie, session, "SELECT \"a\", \"A\" FROM case_distinct");
        let cte = run(
            cassie,
            session,
            "WITH exact AS (SELECT * FROM case_distinct) \
                 SELECT \"a\", \"A\" FROM exact",
        );

        // Assert
        let expected = vec![vec![Value::Int64(1), Value::Int64(2)]];
        assert_eq!(selected.rows(), expected);
        assert_eq!(cte.rows(), expected);
    });
}

#[test]
fn should_resolve_case_distinct_delimited_join_columns_with_case_insensitive_qualifiers() {
    with_users_table("quoted_ident_case_distinct_join", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE quoted_join_left (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "CREATE TABLE quoted_join_right (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO quoted_join_left (\"a\", \"A\") VALUES \
                 (10, 1), (11, 2), (12, 10)",
        );
        run(
            cassie,
            session,
            "INSERT INTO quoted_join_right (\"a\", \"A\") VALUES \
                 (1, 20), (2, 21), (10, 22), (20, 99)",
        );

        // Act
        let joined = run(
            cassie,
            session,
            "SELECT quoted_join_left.\"a\", QUOTED_JOIN_LEFT.\"A\", \
                 quoted_join_right.\"a\", QUOTED_JOIN_RIGHT.\"A\" \
                 FROM quoted_join_left JOIN quoted_join_right \
                 ON quoted_join_left.\"A\" = QUOTED_JOIN_RIGHT.\"a\" \
                 ORDER BY quoted_join_left.\"a\"",
        );

        // Assert
        assert_eq!(
            joined.rows(),
            vec![
                vec![
                    Value::Int64(10),
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::Int64(20),
                ],
                vec![
                    Value::Int64(11),
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::Int64(21),
                ],
                vec![
                    Value::Int64(12),
                    Value::Int64(10),
                    Value::Int64(10),
                    Value::Int64(22),
                ],
            ]
        );
    });
}

#[test]
fn should_score_case_distinct_delimited_text_fields_independently() {
    with_users_table("quoted_ident_case_distinct_fulltext", |cassie, session| {
        // Arrange
        create_case_distinct_fulltext_fixture(cassie, session);

        // Act
        let scored = run(
            cassie,
            session,
            "SELECT id, search_score(\"body\", 'amber') AS lower_score, \
                 search_score(\"Body\", 'amber') AS upper_score \
                 FROM fulltext_case_fields ORDER BY id",
        );

        // Assert
        assert_eq!(scored.rows().len(), 4);
        assert_eq!(scored.rows()[0][0], Value::Int64(1));
        assert!(matches!(scored.rows()[0][1], Value::Float64(score) if score > 0.0));
        assert_eq!(scored.rows()[0][2], Value::Float64(0.0));
        assert_eq!(scored.rows()[1][0], Value::Int64(2));
        assert_eq!(scored.rows()[1][1], Value::Float64(0.0));
        assert!(
            matches!(scored.rows()[1][2], Value::Float64(score) if score > 0.0),
            "expected the exact Body score to match amber"
        );
        let Value::Float64(lower_score) = scored.rows()[2][1] else {
            panic!("expected lower-field full-text score");
        };
        let Value::Float64(upper_score) = scored.rows()[2][2] else {
            panic!("expected upper-field full-text score");
        };
        assert!(lower_score > 0.0);
        assert!((upper_score - lower_score * 3.0).abs() <= f64::EPSILON);
    });
}

#[test]
fn should_search_case_distinct_delimited_text_fields_independently() {
    with_users_table(
        "quoted_ident_case_distinct_fulltext_search",
        |cassie, session| {
            // Arrange
            create_case_distinct_fulltext_fixture(cassie, session);

            // Act
            let matched_upper = run(
                cassie,
                session,
                "SELECT id FROM fulltext_case_fields WHERE search(\"Body\", 'amber') \
                 ORDER BY id",
            );
            let top_upper = run(
                cassie,
                session,
                "SELECT id, search_score(\"Body\", 'amber') AS score \
                 FROM fulltext_case_fields WHERE search(\"Body\", 'amber') \
                 ORDER BY score DESC LIMIT 1",
            );

            // Assert
            assert_eq!(
                matched_upper.rows(),
                vec![
                    vec![Value::Int64(2)],
                    vec![Value::Int64(3)],
                    vec![Value::Int64(4)]
                ]
            );
            assert_eq!(top_upper.rows()[0][0], Value::Int64(4));
            assert!(matches!(top_upper.rows()[0][1], Value::Float64(score) if score > 0.0));
        },
    );
}

#[test]
fn should_group_case_distinct_delimited_columns_independently() {
    with_users_table("quoted_ident_group_case_distinct", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE grouped_case_distinct (\"a\" TEXT, \"A\" TEXT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO grouped_case_distinct (\"a\", \"A\") VALUES \
                 ('north', 'east'), ('north', 'west'), ('north', 'east'), ('south', 'east')",
        );

        // Act
        let grouped = run(
            cassie,
            session,
            "SELECT \"a\", \"A\", count(*) FROM grouped_case_distinct \
                 GROUP BY \"a\", \"A\" ORDER BY \"a\", \"A\"",
        );

        // Assert
        assert_eq!(
            grouped.rows(),
            vec![
                vec![
                    Value::String("north".to_string()),
                    Value::String("east".to_string()),
                    Value::Int64(2),
                ],
                vec![
                    Value::String("north".to_string()),
                    Value::String("west".to_string()),
                    Value::Int64(1),
                ],
                vec![
                    Value::String("south".to_string()),
                    Value::String("east".to_string()),
                    Value::Int64(1),
                ],
            ]
        );
    });
}

#[test]
fn should_rename_only_the_exact_delimited_column() {
    with_users_table("quoted_ident_exact_rename", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE rename_case (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO rename_case (\"a\", \"A\") VALUES (1, 2)",
        );

        // Act
        run(
            cassie,
            session,
            "ALTER TABLE rename_case RENAME COLUMN \"A\" TO \"Upper\"",
        );
        let selected = run(cassie, session, "SELECT \"a\", \"Upper\" FROM rename_case");
        let old_upper_name = cassie.execute_sql(session, "SELECT \"A\" FROM rename_case", vec![]);

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::Int64(1), Value::Int64(2)]]
        );
        assert!(old_upper_name.is_err());
    });
}
