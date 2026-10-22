use cassie::app::Cassie;
use cassie::types::Value;

fn string(value: &str) -> Value {
    Value::String(value.to_string())
}

fn ids(cassie: &Cassie, sql: &str, param: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, vec![string(param)])
        .expect("parameterized query")
        .rows()
        .to_vec()
}

#[test]
fn should_match_bound_string_parameters_like_inline_typed_literals() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE typed_params (id TEXT, v UUID, b BYTEA, ts TIMESTAMP, iv UUID)",
            "CREATE INDEX typed_params_iv ON typed_params (iv)",
            "INSERT INTO typed_params (id, v, b, ts, iv) VALUES ('a', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef', '2024-01-01 12:00:00', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed typed parameter table");
        }
    let upper_uuid = "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11";

    // Act
    let by_uuid = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE v = $1",
        upper_uuid,
    );
    let by_indexed_uuid = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE iv = $1",
        upper_uuid,
    );
    let by_bytea = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE b = $1",
        "\\xDEADBEEF",
    );
    let by_reversed_uuid = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE $1 = v",
        upper_uuid,
    );
    let by_uuid_list = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE v IN ($1, 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')",
        upper_uuid,
    );
    let by_timestamp = ids(
        &cassie,
        "SELECT id FROM typed_params WHERE ts = $1",
        "2024-01-01 12:00:00",
    );

    // Assert
    let expected = vec![vec![string("a")]];
    assert_eq!(by_uuid, expected);
    assert_eq!(by_indexed_uuid, expected);
    assert_eq!(by_reversed_uuid, expected);
    assert_eq!(by_uuid_list, expected);
    assert_eq!(by_bytea, expected);
    assert_eq!(by_timestamp, expected);
    drop(session);
    drop(cassie);
}
