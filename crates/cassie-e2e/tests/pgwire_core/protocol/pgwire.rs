use cassie::pgwire::protocol::{decode, encode, ClientMessage, RowDescriptionField, ServerMessage};

#[test]
fn should_decode_basic_protocol_messages() {
    // Arrange
    let startup = decode("STARTUP user=alice database=testdb");
    let query = decode("QUERY select 1");

    // Act
    let encoded = encode(&ServerMessage::RowDescription(vec![
        RowDescriptionField {
            name: "id".to_string(),
            data_type: "text".to_string(),
            type_oid: 25,
            typlen: -1,
            atttypmod: -1,
            format_code: 0,
            nullable: true,
        },
        RowDescriptionField {
            name: "score".to_string(),
            data_type: "float".to_string(),
            type_oid: 701,
            typlen: 8,
            atttypmod: -1,
            format_code: 0,
            nullable: true,
        },
    ]));
    let raw = String::from_utf8_lossy(&encoded);
    let payload = raw
        .strip_prefix("ROWDESC ")
        .and_then(|value| value.strip_suffix('\n'))
        .unwrap_or_default();
    let decoded: Vec<RowDescriptionField> =
        serde_json::from_str(payload).expect("row description payload should be valid json");

    // Assert
    let ClientMessage::Startup { user, database } = startup else {
        panic!("expected startup message");
    };
    assert_eq!(user, "alice");
    assert_eq!(database, Some("testdb".to_string()));

    let ClientMessage::Query(sql) = query else {
        panic!("expected query message");
    };
    assert_eq!(sql, "select 1");
    assert_eq!(decoded[0].name, "id");
    assert_eq!(decoded[1].type_oid, 701);
    assert_eq!(encoded.last(), Some(&b'\n'));
}

#[test]
fn should_decode_extended_protocol_lifecycle_messages() {
    // Arrange
    let parse = decode("PARSE q1|SELECT * FROM items");
    let bind = decode("BIND q1 $1|$2");
    let describe = decode("DESCRIBE q1");
    let execute = decode("EXECUTE q1 3");
    let close = decode("CLOSE q1");

    // Act
    let ClientMessage::Parse { name, query } = parse else {
        panic!("expected parse message");
    };

    // Assert
    assert_eq!(name, "q1");
    assert_eq!(query, "SELECT * FROM items");

    let ClientMessage::Bind { name, params } = bind else {
        panic!("expected bind message");
    };
    assert_eq!(name, "q1");
    assert_eq!(params, vec!["$1", "$2"]);

    let ClientMessage::Describe(name) = describe else {
        panic!("expected describe message");
    };
    assert_eq!(name, "q1");

    let ClientMessage::Execute { name, limit } = execute else {
        panic!("expected execute message");
    };
    assert_eq!(name, "q1");
    assert_eq!(limit, Some(3));

    let ClientMessage::Close(name) = close else {
        panic!("expected close message");
    };
    assert_eq!(name, "q1");
}
