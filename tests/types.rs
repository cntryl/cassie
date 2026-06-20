use cassie::app::Cassie;
use cassie::types::{DataType, Value, Vector};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-types-{}", label));
    path.push(Uuid::new_v4().to_string());
    path.to_string_lossy().to_string()
}

#[test]
fn should_parse_supported_scalar_sql_types() {
    // Arrange
    // Act
    let int = DataType::parse_sql("INT").unwrap();
    let smallint = DataType::parse_sql("SMALLINT").unwrap();
    let bigint = DataType::parse_sql("BIGINT").unwrap();
    let vector = DataType::parse_sql("vector(2)").unwrap();
    let json = DataType::parse_sql("json").unwrap();
    let uuid = DataType::parse_sql("uuid").unwrap();
    let char = DataType::parse_sql("char(3)").unwrap();
    let varchar = DataType::parse_sql("varchar(12)").unwrap();
    let bytea = DataType::parse_sql("bytea").unwrap();

    // Assert
    assert_eq!(int, DataType::Int);
    assert_eq!(smallint, DataType::SmallInt);
    assert_eq!(bigint, DataType::BigInt);
    assert_eq!(vector, DataType::Vector(2));
    assert_eq!(json, DataType::Json);
    assert_eq!(uuid, DataType::Uuid);
    assert_eq!(char, DataType::Char { length: Some(3) });
    assert_eq!(varchar, DataType::Varchar { length: Some(12) });
    assert_eq!(bytea, DataType::Bytea);
}

#[test]
fn should_parse_supported_array_sql_types() {
    // Arrange
    // Act
    let array = DataType::parse_sql("text[]").unwrap();
    let unsupported = DataType::parse_sql("int[][]");

    // Assert
    assert_eq!(array, DataType::Array(Box::new(DataType::Text)));
    assert!(unsupported.is_err());
}

#[test]
fn should_validate_deterministic_type_oid_assignments() {
    // Arrange
    let vector_two = DataType::Vector(2);
    let vector_three = DataType::Vector(3);
    let int_array = DataType::Array(Box::new(DataType::Int));

    // Act
    let vector_two_oid = vector_two.type_oid();
    let vector_three_oid = vector_three.type_oid();
    let int_array_oid = int_array.type_oid();

    // Assert
    assert_eq!(vector_two_oid, vector_three_oid - 1);
    assert_eq!(int_array_oid, 34000 + DataType::Int.type_oid() % 10000);
}

#[test]
fn should_roundtrip_supported_sql_values() {
    // Arrange
    with_fallback();
    let path = data_dir("roundtrip");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE type_round_trip (item_id TEXT, item_uuid UUID, created_on DATE, created_at TIME, created_at_ts TIMESTAMP, payload JSON, values INT[], embedding VECTOR(2), short SMALLINT, wide BIGINT, code CHAR(4), title VARCHAR(10), blob BYTEA)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO type_round_trip (item_id, item_uuid, created_on, created_at, created_at_ts, payload, values, embedding, short, wide, code, title, blob) VALUES ('row-1', $1, $2, $3, $4, $5, $6, $7, $8, $9, 'ABCD', 'sample', '\\x01020a')",
                vec![
                    Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
                    Value::String("2026-06-18".to_string()),
                    Value::String("12:34:56".to_string()),
                    Value::String("2026-06-18T12:34:56Z".to_string()),
                    Value::Json(serde_json::json!({"source": "types", "value": 1})),
                    Value::Json(serde_json::json!([1, 2, 3])),
                    Value::Json(serde_json::json!([0.25, 0.75])),
                    Value::Int64(12),
                    Value::Int64(9_223_372_036_854_775_807),
                ],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT item_id, item_uuid, created_on, created_at, created_at_ts, payload, values, embedding, short, wide, code, title, blob FROM type_round_trip WHERE item_id = 'row-1'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("row-1".to_string()));
        assert_eq!(
            selected.rows[0][1],
            Value::String("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(selected.rows[0][2], Value::String("2026-06-18".to_string()));
        assert_eq!(selected.rows[0][3], Value::String("12:34:56".to_string()));
        assert_eq!(selected.rows[0][4], Value::String("2026-06-18T12:34:56Z".to_string()));
        assert_eq!(
            selected.rows[0][5],
            Value::Json(serde_json::json!({"source": "types", "value": 1}))
        );
        assert_eq!(
            selected.rows[0][6],
            Value::Json(serde_json::json!([1, 2, 3]))
        );
        assert_eq!(
            selected.rows[0][7],
            Value::Vector(Vector::new(vec![0.25, 0.75]))
        );
        assert_eq!(selected.rows[0][8], Value::Int64(12));
        assert_eq!(
            selected.rows[0][9],
            Value::Int64(9_223_372_036_854_775_807)
        );
        assert_eq!(selected.rows[0][10], Value::String("ABCD".to_string()));
        assert_eq!(selected.rows[0][11], Value::String("sample".to_string()));
        assert_eq!(selected.rows[0][12], Value::String("\\x01020a".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cast_string_to_uuid() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let casted_uuid = cassie
            .execute_sql(
                &session,
                "SELECT CAST('550e8400-e29b-41d4-a716-446655440000' AS UUID)",
                vec![],
            )
            .unwrap();
        // Assert
        assert_eq!(
            casted_uuid.rows[0][0],
            Value::String("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
    });
}

#[test]
fn should_cast_null_to_text() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let casted_text = cassie
            .execute_sql(&session, "SELECT CAST(NULL AS TEXT)", vec![])
            .unwrap();

        // Assert
        assert_eq!(casted_text.rows[0][0], Value::Null);
    });
}

#[test]
fn should_fail_when_casting_scalar_to_unsupported_type_family() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let vector_cast = cassie.execute_sql(&session, "SELECT CAST(1 AS VECTOR(2))", vec![]);
        let array_cast = cassie.execute_sql(&session, "SELECT CAST(1 AS INT[])", vec![]);

        // Assert
        assert!(vector_cast.is_err(), "vector cast should be unsupported");
        assert!(array_cast.is_err(), "array cast should be unsupported");
        if let Err(error) = vector_cast {
            assert!(
                error
                    .to_string()
                    .contains("cannot cast scalar value to VECTOR"),
                "unexpected vector cast error: {error}"
            );
        }
        if let Err(error) = array_cast {
            assert!(
                error
                    .to_string()
                    .contains("cannot cast scalar value to ARRAY"),
                "unexpected array cast error: {error}"
            );
        }
    });
}
