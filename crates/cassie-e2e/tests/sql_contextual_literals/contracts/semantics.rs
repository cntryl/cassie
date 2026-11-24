#[test]
fn should_compare_jsonb_column_with_json_string_literal() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE jsonb_literal_probe (id TEXT, doc JSONB)",
            vec![],
        )
        .expect("create JSONB table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO jsonb_literal_probe VALUES ('match', $1), ('other', $2), ('array', $3)",
            vec![
                Value::Json(serde_json::json!({"k": 1})),
                Value::Json(serde_json::json!({"k": 2})),
                Value::Json(serde_json::json!([1, 2])),
            ],
        )
        .expect("seed JSONB table");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_literal_probe WHERE doc = '{ \"k\" : 1 }'",
            vec![],
        )
        .expect("compare JSONB with JSON string literal");
    let reversed = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_literal_probe WHERE '{ \"k\" : 1 }' = doc",
            vec![],
        )
        .expect("compare JSON string literal with JSONB");
    let inequality = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_literal_probe WHERE doc <> '{\"k\":2}' ORDER BY id",
            vec![],
        )
        .expect("compare JSONB inequality with JSON string literal");
    let array_result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_literal_probe WHERE doc IN ('[1,2]')",
            vec![],
        )
        .expect("compare JSONB with array-shaped JSON string literal");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("match".to_string())]]
    );
    assert_eq!(reversed.rows(), result.rows());
    assert_eq!(
        inequality.rows(),
        vec![
            vec![Value::String("array".to_string())],
            vec![Value::String("match".to_string())]
        ]
    );
    assert_eq!(
        array_result.rows(),
        vec![vec![Value::String("array".to_string())]]
    );
}

#[test]
fn should_compare_jsonb_scalar_string_literals() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE jsonb_scalar_literal_probe (id TEXT, doc JSONB)",
            vec![],
        )
        .expect("create JSONB table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO jsonb_scalar_literal_probe VALUES ('number', $1), ('boolean', $2), ('sql-null', NULL)",
            vec![
                Value::Json(serde_json::json!(7)),
                Value::Json(serde_json::json!(true)),
            ],
        )
        .expect("seed JSONB scalar values");

    // Act
    let number = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_scalar_literal_probe WHERE doc = '7'",
            vec![],
        )
        .expect("compare JSONB number with JSON string literal");
    let boolean = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_scalar_literal_probe WHERE doc = 'true'",
            vec![],
        )
        .expect("compare JSONB boolean with JSON string literal");
    let sql_null = cassie
        .execute_sql(
            &session,
            "SELECT id FROM jsonb_scalar_literal_probe WHERE doc = NULL",
            vec![],
        )
        .expect("compare JSONB values with SQL NULL");
    let projected = cassie
        .execute_sql(
            &session,
            "SELECT doc = '7.0' AS matches FROM jsonb_scalar_literal_probe WHERE id = 'number'",
            vec![],
        )
        .expect("project JSONB equality");
    let updated = cassie
        .execute_sql(
            &session,
            "UPDATE jsonb_scalar_literal_probe SET id = 'updated' WHERE doc = '7' RETURNING id",
            vec![],
        )
        .expect("update through JSONB equality");
    let deleted = cassie
        .execute_sql(
            &session,
            "DELETE FROM jsonb_scalar_literal_probe WHERE doc = 'true' RETURNING id",
            vec![],
        )
        .expect("delete through JSONB equality");

    // Assert
    assert_eq!(
        number.rows(),
        vec![vec![Value::String("number".to_string())]]
    );
    assert_eq!(
        boolean.rows(),
        vec![vec![Value::String("boolean".to_string())]]
    );
    assert_eq!(sql_null.rows(), [] as [Vec<Value>; 0]);
    assert_eq!(projected.rows(), vec![vec![Value::Bool(true)]]);
    assert_eq!(
        updated.rows(),
        vec![vec![Value::String("updated".to_string())]]
    );
    assert_eq!(
        deleted.rows(),
        vec![vec![Value::String("boolean".to_string())]]
    );
}

#[test]
fn should_compare_text_with_array_shaped_string_literal() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE text_array_literal_probe (id TEXT, label TEXT)",
            vec![],
        )
        .expect("create TEXT table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO text_array_literal_probe VALUES ('array', '[1,2]'), ('word', 'hello')",
            vec![],
        )
        .expect("seed TEXT table");

    // Act
    let equality = cassie
        .execute_sql(
            &session,
            "SELECT id FROM text_array_literal_probe WHERE label = '[1,2]'",
            vec![],
        )
        .expect("compare TEXT with array-shaped literal");
    let inequality = cassie
        .execute_sql(
            &session,
            "SELECT id FROM text_array_literal_probe WHERE label <> '[1,2]' ORDER BY id",
            vec![],
        )
        .expect("compare inequality with array-shaped literal");
    let in_list = cassie
        .execute_sql(
            &session,
            "SELECT id FROM text_array_literal_probe WHERE label IN ('[1,2]', 'missing')",
            vec![],
        )
        .expect("compare IN with array-shaped literal");

    // Assert
    assert_eq!(
        equality.rows(),
        vec![vec![Value::String("array".to_string())]]
    );
    assert_eq!(
        inequality.rows(),
        vec![vec![Value::String("word".to_string())]]
    );
    assert_eq!(
        in_list.rows(),
        vec![vec![Value::String("array".to_string())]]
    );
}

fn typed_literal_probe() -> (cassie::app::Cassie, cassie::app::CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE typed_literal_probe (id INT PRIMARY KEY, small SMALLINT, big BIGINT, \
         ratio FLOAT8, amount NUMERIC, numbers INT[])",
        "INSERT INTO typed_literal_probe VALUES (1, 3, 9, 2.5, 1.5, ARRAY[1,2]), \
         (2, 4, 10, 3.5, 2.5, ARRAY[3])",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("seed typed literal probe");
    }
    (cassie, session)
}

#[test]
fn should_compare_typed_columns_with_quoted_literals() {
    // Arrange
    let (cassie, session) = typed_literal_probe();

    // Act
    let matched = [
        "id = '1'",
        "'1' = id",
        "small = '3'",
        "big = '9'",
        "ratio = '2.5'",
        "amount = '1.5'",
        "numbers = '{1,2}'",
        "id < '2'",
        "id IN ('1', '7')",
        "id BETWEEN '0' AND '1'",
        "id IS NOT DISTINCT FROM '1'",
    ]
    .map(|predicate| {
        cassie
            .execute_sql(
                &session,
                &format!("SELECT id FROM typed_literal_probe WHERE {predicate}"),
                vec![],
            )
            .map(|result| result.rows().to_vec())
    });

    // Assert
    for rows in matched {
        assert!(rows.is_ok_and(|rows| rows == [vec![Value::Int64(1)]]));
    }
}

#[test]
fn should_apply_quoted_literal_predicates_when_updating_and_deleting() {
    // Arrange
    let (cassie, session) = typed_literal_probe();

    // Act
    let updated = cassie.execute_sql(
        &session,
        "UPDATE typed_literal_probe SET ratio = 9.5 WHERE small = '3'",
        vec![],
    );
    let deleted = cassie.execute_sql(
        &session,
        "DELETE FROM typed_literal_probe WHERE big = '10'",
        vec![],
    );
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, ratio FROM typed_literal_probe ORDER BY id",
            vec![],
        )
        .map(|result| result.rows().to_vec());

    // Assert
    assert!(updated.is_ok(), "UPDATE accepts the quoted literal");
    assert!(deleted.is_ok(), "DELETE accepts the quoted literal");
    assert!(rows.is_ok_and(|rows| rows == [vec![Value::Int64(1), Value::Float64(9.5)]]));
}

#[test]
fn should_reject_a_quoted_literal_that_is_not_valid_for_the_compared_column() {
    // Arrange
    let (cassie, session) = typed_literal_probe();

    // Act
    let error = cassie.execute_sql(
        &session,
        "SELECT id FROM typed_literal_probe WHERE id = 'x'",
        vec![],
    );

    // Assert
    assert!(error.is_err_and(|error| error.sql_state() == "22P02"));
}
