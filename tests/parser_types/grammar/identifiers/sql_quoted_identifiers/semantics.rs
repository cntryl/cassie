#[test]
fn should_resolve_a_quoted_identifier_in_where_to_the_column() {
    with_users_table("quoted_ident_where", |cassie, session| {
        // Arrange
        let sql = "SELECT name FROM users WHERE \"name\" = 'alice'";

        // Act
        let selected = run(cassie, session, sql);

        // Assert
        assert_eq!(selected.rows(), string_rows(&["alice"]));
    });
}

#[test]
fn should_project_a_quoted_identifier_as_the_column_value() {
    with_users_table("quoted_ident_projection", |cassie, session| {
        // Arrange
        let sql = "SELECT \"name\" FROM users ORDER BY \"id\" DESC";

        // Act
        let selected = run(cassie, session, sql);

        // Assert
        assert_eq!(selected.columns()[0].name, "name");
        assert_eq!(selected.rows(), string_rows(&["bob", "alice"]));
    });
}

#[test]
fn should_update_a_quoted_target_on_rows_matched_by_a_quoted_identifier() {
    with_users_table("quoted_ident_update", |cassie, session| {
        // Arrange
        let update = "UPDATE users SET \"name\" = 'x' WHERE \"id\" = 1";

        // Act
        let updated = run(cassie, session, update);
        let remaining = run(cassie, session, "SELECT name FROM users ORDER BY id");

        // Assert
        assert_eq!(updated.command(), "UPDATE 1");
        assert_eq!(remaining.rows(), string_rows(&["x", "bob"]));
    });
}

#[test]
fn should_delete_rows_matched_by_a_quoted_identifier() {
    with_users_table("quoted_ident_delete", |cassie, session| {
        // Arrange
        let delete = "DELETE FROM users WHERE \"id\" = 2";

        // Act
        let deleted = run(cassie, session, delete);
        let remaining = run(cassie, session, "SELECT name FROM users ORDER BY id");

        // Assert
        assert_eq!(deleted.command(), "DELETE 1");
        assert_eq!(remaining.rows(), string_rows(&["alice"]));
    });
}

#[test]
fn should_insert_into_a_quoted_column_created_by_create_table() {
    with_users_table("quoted_ident_insert_columns", |cassie, session| {
        // Arrange
        run(cassie, session, "CREATE TABLE q (\"Col\" TEXT)");

        // Act
        let inserted = run(cassie, session, "INSERT INTO q (\"Col\") VALUES ('v')");
        let selected = run(cassie, session, "SELECT \"Col\" FROM q");

        // Assert
        assert_eq!(inserted.command(), "INSERT 0 1");
        assert_eq!(selected.rows(), string_rows(&["v"]));
    });
}

#[test]
fn should_strip_quotes_from_a_quoted_column_alias() {
    with_users_table("quoted_ident_alias", |cassie, session| {
        // Arrange
        let sql = "SELECT name AS \"Foo\" FROM users WHERE id = 1";

        // Act
        let selected = run(cassie, session, sql);

        // Assert
        assert_eq!(selected.columns()[0].name, "Foo");
        assert_eq!(selected.rows(), string_rows(&["alice"]));
    });
}

#[test]
fn should_arbitrate_on_conflict_with_a_quoted_target_column() {
    with_users_table("quoted_ident_on_conflict", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE keyed (id BIGINT PRIMARY KEY, v TEXT)",
        );
        run(cassie, session, "INSERT INTO keyed (id, v) VALUES (1, 'a')");

        // Act
        let upserted = run(
                cassie,
                session,
                "INSERT INTO keyed (\"id\", \"v\") VALUES (1, 'b') ON CONFLICT (\"id\") DO UPDATE SET \"v\" = 'c'",
            );
        let selected = run(cassie, session, "SELECT v FROM keyed");

        // Assert
        assert_eq!(upserted.command(), "INSERT 0 1");
        assert_eq!(selected.rows(), string_rows(&["c"]));
    });
}

#[test]
fn should_update_case_distinct_columns_from_the_matching_excluded_fields() {
    with_users_table("quoted_ident_on_conflict_excluded", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE conflict_case_distinct (\"key\" TEXT UNIQUE, \"a\" TEXT, \"A\" TEXT)",
        );
        run(
                cassie,
                session,
                "INSERT INTO conflict_case_distinct (\"key\", \"a\", \"A\") VALUES ('k', 'lower-old', 'upper-old')",
            );

        // Act
        run(
                cassie,
                session,
                "INSERT INTO conflict_case_distinct (\"key\", \"a\", \"A\") VALUES ('k', 'lower-new', 'upper-new') \
                 ON CONFLICT (\"key\") DO UPDATE SET \"a\" = excluded.\"a\", \"A\" = excluded.\"A\"",
            );
        let selected = run(
            cassie,
            session,
            "SELECT \"a\", \"A\" FROM conflict_case_distinct",
        );

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![
                Value::String("lower-new".to_string()),
                Value::String("upper-new".to_string()),
            ]]
        );
    });
}

#[test]
fn should_group_by_a_quoted_identifier_as_the_column_value() {
    with_users_table("quoted_ident_group", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "INSERT INTO users (id, name) VALUES (3, 'bob')",
        );
        let sql = "SELECT \"name\", count(*) FROM users GROUP BY \"name\" ORDER BY \"name\"";

        // Act
        let grouped = run(cassie, session, sql);

        // Assert
        assert_eq!(
            grouped.rows(),
            vec![
                vec![Value::String("alice".to_string()), Value::Int64(1)],
                vec![Value::String("bob".to_string()), Value::Int64(2)],
            ]
        );
    });
}

#[test]
fn should_compare_a_quoted_identifier_against_a_numeric_literal() {
    with_users_table("quoted_ident_compare", |cassie, session| {
        // Arrange
        let sql = "SELECT name FROM users WHERE \"id\" > 1";

        // Act
        let compared = run(cassie, session, sql);

        // Assert
        assert_eq!(compared.rows(), string_rows(&["bob"]));
    });
}

#[test]
fn should_resolve_quoted_identifiers_containing_spaces_or_multibyte_characters() {
    with_users_table("quoted_ident_spaces", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE people (\"first name\" TEXT, \"café\" TEXT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO people (\"first name\", \"café\") VALUES ('ada', 'noir')",
        );
        let sql = "SELECT \"café\" FROM people WHERE \"first name\" = 'ada'";

        // Act
        let selected = run(cassie, session, sql);

        // Assert
        assert_eq!(selected.columns()[0].name, "café");
        assert_eq!(selected.rows(), string_rows(&["noir"]));
    });
}

#[test]
fn should_parse_quoted_qualified_identifiers_as_column_paths() {
    // Arrange
    let cases = [
        ("SELECT \"users\".\"name\" FROM users", "users.name"),
        ("SELECT users.\"name\" FROM users", "users.name"),
        ("SELECT \"we\"\"ird\" FROM users", "\"we\"\"ird\""),
    ];

    // Act
    let parsed = cases.map(|(sql, expected)| {
        let statement = parse_statement(sql).expect("parse quoted identifier");
        let QueryStatement::Select(select) = statement.statement else {
            panic!("expected SELECT statement for {sql}");
        };
        (select.projection[0].clone(), expected)
    });

    // Assert
    for (item, expected) in parsed {
        let SelectItem::Column { name, .. } = item else {
            panic!("expected column projection for {expected}");
        };
        assert_eq!(name, expected);
    }
}

#[test]
fn should_preserve_case_distinct_text_statistics_through_a_join() {
    with_users_table(
        "quoted_ident_case_distinct_join_fulltext",
        |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE join_fulltext_left (id INT, \"body\" TEXT, \"Body\" TEXT)",
            );
            run(
                cassie,
                session,
                "CREATE TABLE join_fulltext_right (left_id INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO join_fulltext_left (id, \"body\", \"Body\") VALUES \
                 (1, 'amber amber', 'violet'), (2, 'violet', 'amber amber')",
            );
            run(
                cassie,
                session,
                "INSERT INTO join_fulltext_right (left_id) VALUES (1), (2)",
            );

            // Act
            let matched = run(
                cassie,
                session,
                "SELECT join_fulltext_left.id, search_score(\"Body\", 'amber') AS score \
                 FROM join_fulltext_left JOIN join_fulltext_right \
                 ON join_fulltext_left.id = join_fulltext_right.left_id \
                 WHERE search(\"Body\", 'amber') ORDER BY join_fulltext_left.id",
            );

            // Assert
            assert_eq!(matched.rows().len(), 1);
            assert_eq!(matched.rows()[0][0], Value::Int64(2));
            assert!(matches!(matched.rows()[0][1], Value::Float64(score) if score > 0.0));
        },
    );
}

#[test]
fn should_preserve_exact_spelling_in_copy_column_references() {
    // Arrange
    let sql = "COPY exact_case (\"Email\") FROM STDIN";

    // Act
    let parsed = parse_statement(sql).expect("parse COPY column identifier");

    // Assert
    let QueryStatement::Copy(statement) = parsed.statement else {
        panic!("expected COPY statement");
    };
    assert_eq!(statement.columns, vec!["\"Email\""]);
}

#[test]
fn should_copy_into_case_distinct_columns_independently() {
    with_users_table("quoted_ident_copy_case_distinct", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE copy_case_distinct (\"a\" INT, \"A\" INT)",
        );
        let parsed =
            parse_statement("COPY copy_case_distinct (\"a\", \"A\") FROM STDIN WITH (FORMAT csv)")
                .expect("parse COPY statement");
        let QueryStatement::Copy(statement) = parsed.statement else {
            panic!("expected COPY statement");
        };

        // Act
        let copied = cassie
            .copy_from_csv_stdin(session, &statement, b"11,22\n")
            .expect("copy case-distinct fields");
        let selected = run(
            cassie,
            session,
            "SELECT \"a\", \"A\" FROM copy_case_distinct",
        );

        // Assert
        assert_eq!(copied, 1);
        assert_eq!(
            selected.rows(),
            vec![vec![Value::Int64(11), Value::Int64(22)]]
        );
    });
}

#[test]
fn should_resolve_correlated_case_distinct_columns_independently() {
    with_users_table("quoted_ident_correlated_exists", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE exact_outer (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO exact_outer (\"a\", \"A\") VALUES (1, 2)",
        );
        run(
            cassie,
            session,
            "CREATE TABLE exact_inner (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO exact_inner (\"a\", \"A\") VALUES (1, 2)",
        );

        // Act
        let mismatched = run(
            cassie,
            session,
            "SELECT exact_outer.\"a\" FROM exact_outer WHERE EXISTS \
                 (SELECT 1 FROM exact_inner WHERE exact_inner.\"A\" = exact_outer.\"a\")",
        );
        let exact = run(
            cassie,
            session,
            "SELECT exact_outer.\"a\" FROM exact_outer WHERE EXISTS \
                 (SELECT 1 FROM exact_inner WHERE exact_inner.\"A\" = exact_outer.\"A\")",
        );

        // Assert
        assert_eq!(mismatched.rows(), Vec::<Vec<Value>>::new());
        assert_eq!(exact.rows(), vec![vec![Value::Int64(1)]]);
    });
}

#[test]
fn should_filter_case_distinct_columns_independently() {
    with_users_table("quoted_ident_case_distinct_filter", |cassie, session| {
        // Arrange
        create_case_distinct_dml_fixture(cassie, session);

        // Act
        let filtered = run(
            cassie,
            session,
            "SELECT \"a\" FROM separate_case WHERE \"A\" = 2",
        );

        // Assert
        assert_eq!(filtered.rows(), vec![vec![Value::Int64(1)]]);
    });
}

#[test]
fn should_update_case_distinct_columns_independently() {
    with_users_table("quoted_ident_case_distinct_update", |cassie, session| {
        // Arrange
        create_case_distinct_dml_fixture(cassie, session);

        // Act
        let updated = run(
            cassie,
            session,
            "UPDATE separate_case SET \"A\" = 3 WHERE \"a\" = 1",
        );
        let selected = run(cassie, session, "SELECT \"a\", \"A\" FROM separate_case");

        // Assert
        assert_eq!(updated.command(), "UPDATE 1");
        assert_eq!(
            selected.rows(),
            vec![vec![Value::Int64(1), Value::Int64(3)]]
        );
    });
}

#[test]
fn should_enforce_unique_constraints_on_case_distinct_columns_independently() {
    with_users_table("quoted_ident_case_distinct_unique", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE unique_case (\"a\" INT UNIQUE, \"A\" INT UNIQUE)",
        );
        run(
            cassie,
            session,
            "INSERT INTO unique_case (\"a\", \"A\") VALUES (1, 2)",
        );

        // Act
        let duplicate_lower = cassie.execute_sql(
            session,
            "INSERT INTO unique_case (\"a\", \"A\") VALUES (1, 3)",
            vec![],
        );
        let duplicate_upper = cassie.execute_sql(
            session,
            "INSERT INTO unique_case (\"a\", \"A\") VALUES (4, 2)",
            vec![],
        );

        // Assert
        assert!(duplicate_lower.is_err());
        assert!(duplicate_upper.is_err());
    });
}

#[test]
fn should_not_use_an_index_on_a_case_distinct_sibling_column() {
    with_users_table("quoted_ident_case_distinct_index", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "CREATE TABLE indexed_case (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO indexed_case (\"a\", \"A\") VALUES (1, 20)",
        );
        run(
            cassie,
            session,
            "INSERT INTO indexed_case (\"a\", \"A\") VALUES (2, 10)",
        );
        run(
            cassie,
            session,
            "CREATE INDEX indexed_case_upper_idx ON indexed_case (\"A\")",
        );

        // Act
        let sibling_plan = run(
            cassie,
            session,
            "EXPLAIN SELECT \"a\" FROM indexed_case WHERE \"a\" = 1",
        );
        let matching_plan = run(
            cassie,
            session,
            "EXPLAIN SELECT \"A\" FROM indexed_case WHERE \"A\" = 10",
        );
        let Value::String(sibling_plan) = &sibling_plan.rows()[0][0] else {
            panic!("expected sibling explain string");
        };
        let Value::String(matching_plan) = &matching_plan.rows()[0][0] else {
            panic!("expected matching explain string");
        };
        let selected = run(
            cassie,
            session,
            "SELECT \"a\" FROM indexed_case WHERE \"a\" = 1",
        );
        let matching = run(
            cassie,
            session,
            "SELECT \"A\" FROM indexed_case WHERE \"A\" = 10",
        );

        // Assert
        assert!(
            sibling_plan.contains("index=none"),
            "the case-distinct sibling should not use the upper-case index"
        );
        assert!(
            matching_plan.contains("index=postgres.public.indexed_case_upper_idx"),
            "the exact-case query should use the matching upper-case index"
        );
        assert_eq!(selected.rows(), vec![vec![Value::Int64(1)]]);
        assert_eq!(matching.rows(), vec![vec![Value::Int64(10)]]);
    });
}
