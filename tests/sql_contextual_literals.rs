#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::Cassie;
use cassie::app::CassieError;
use cassie::types::Value;

use support_sql::{data_dir, use_local_storage};

#[test]
fn should_reject_invalid_json_string_comparands() {
    // Arrange
    use_local_storage();
    let path = data_dir("invalid_json_string_comparands");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE invalid_json_literals (id TEXT, doc JSONB)",
            vec![],
        )
        .expect("create JSONB table");

    // Act
    let results = [
        "SELECT id FROM invalid_json_literals WHERE doc = 'not-json'",
        "SELECT id FROM invalid_json_literals WHERE 'not-json' <> doc",
        "SELECT id FROM invalid_json_literals WHERE doc IN ('{}', 'not-json')",
    ]
    .map(|sql| cassie.execute_sql(&session, sql, vec![]));

    // Assert
    for result in results {
        assert!(
            matches!(result, Err(CassieError::Planner(message)) if message.contains("invalid JSON literal"))
        );
    }
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compare_jsonb_column_with_json_string_literal() {
    // Arrange
    use_local_storage();
    let path = data_dir("jsonb_string_literal_comparison");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
    assert_eq!(result.rows, vec![vec![Value::String("match".to_string())]]);
    assert_eq!(reversed.rows, result.rows);
    assert_eq!(
        inequality.rows,
        vec![
            vec![Value::String("array".to_string())],
            vec![Value::String("match".to_string())]
        ]
    );
    assert_eq!(
        array_result.rows,
        vec![vec![Value::String("array".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compare_jsonb_scalar_string_literals() {
    // Arrange
    use_local_storage();
    let path = data_dir("jsonb_scalar_string_literal_comparison");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
    assert_eq!(number.rows, vec![vec![Value::String("number".to_string())]]);
    assert_eq!(
        boolean.rows,
        vec![vec![Value::String("boolean".to_string())]]
    );
    assert_eq!(sql_null.rows, [] as [Vec<Value>; 0]);
    assert_eq!(projected.rows, vec![vec![Value::Bool(true)]]);
    assert_eq!(
        updated.rows,
        vec![vec![Value::String("updated".to_string())]]
    );
    assert_eq!(
        deleted.rows,
        vec![vec![Value::String("boolean".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_compare_text_with_array_shaped_string_literal() {
    // Arrange
    use_local_storage();
    let path = data_dir("text_array_shaped_literals");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
        equality.rows,
        vec![vec![Value::String("array".to_string())]]
    );
    assert_eq!(
        inequality.rows,
        vec![vec![Value::String("word".to_string())]]
    );
    assert_eq!(in_list.rows, vec![vec![Value::String("array".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}
