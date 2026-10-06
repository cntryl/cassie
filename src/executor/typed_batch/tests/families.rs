use crate::types::{DataType, Value};

pub(super) fn logical_families() -> Vec<(DataType, Vec<Value>)> {
    let mut families = native_families();
    families.extend(scalar_families());
    families
}

fn native_families() -> Vec<(DataType, Vec<Value>)> {
    vec![
        (DataType::Null, vec![Value::Null, Value::Null, Value::Null]),
        (
            DataType::SmallInt,
            vec![
                Value::Int64(i64::from(i16::MIN)),
                Value::Null,
                Value::Int64(i64::from(i16::MAX)),
            ],
        ),
        (
            DataType::Int,
            vec![
                Value::Int64(i64::from(i32::MIN)),
                Value::Null,
                Value::Int64(i64::from(i32::MAX)),
            ],
        ),
        (
            DataType::BigInt,
            vec![
                Value::Int64(i64::MIN),
                Value::Null,
                Value::Int64(9_007_199_254_740_993),
            ],
        ),
        (
            DataType::Float,
            vec![
                Value::Float64(-0.0),
                Value::Null,
                Value::Float64(f64::from_bits(1)),
            ],
        ),
        (
            DataType::Boolean,
            vec![Value::Bool(false), Value::Null, Value::Bool(true)],
        ),
        (
            DataType::Text,
            vec![
                Value::String("λ".to_owned()),
                Value::Null,
                Value::String(String::new()),
            ],
        ),
        (
            DataType::Char { length: Some(3) },
            vec![
                Value::String("a  ".to_owned()),
                Value::Null,
                Value::String("b".to_owned()),
            ],
        ),
        (
            DataType::Varchar { length: Some(7) },
            vec![
                Value::String("λ".to_owned()),
                Value::Null,
                Value::String("text".to_owned()),
            ],
        ),
    ]
}

fn scalar_families() -> Vec<(DataType, Vec<Value>)> {
    vec![
        (
            DataType::Uuid,
            vec![
                Value::String(uuid::Uuid::nil().to_string()),
                Value::Null,
                Value::String(uuid::Uuid::nil().to_string()),
            ],
        ),
        (
            DataType::Bytea,
            vec![
                Value::String("\\x00ff".to_owned()),
                Value::Null,
                Value::String("\\x".to_owned()),
            ],
        ),
        (
            DataType::Date,
            vec![
                Value::String("2000-02-29".to_owned()),
                Value::Null,
                Value::String("2026-10-06".to_owned()),
            ],
        ),
        (
            DataType::Time,
            vec![
                Value::String("23:59:59.123456".to_owned()),
                Value::Null,
                Value::String("00:00:00".to_owned()),
            ],
        ),
        (
            DataType::Timestamp,
            vec![
                Value::String("2026-10-06 00:00:00.123456".to_owned()),
                Value::Null,
                Value::String("2000-01-01 00:00:00".to_owned()),
            ],
        ),
        (
            DataType::Vector(2),
            vec![
                Value::Vector(crate::types::Vector::new(vec![1.0, 2.0])),
                Value::Null,
                Value::Json(serde_json::json!([3, 4])),
            ],
        ),
        (
            DataType::Json,
            vec![
                Value::Int64(9_007_199_254_740_993),
                Value::Null,
                Value::Json(serde_json::Value::Null),
            ],
        ),
        (
            DataType::Array(Box::new(DataType::Char { length: Some(3) })),
            vec![
                Value::Json(serde_json::json!(["x  ", null])),
                Value::Null,
                Value::Json(serde_json::json!([])),
            ],
        ),
    ]
}
