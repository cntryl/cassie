use cassie::app::CassieError;
use cassie::sql::{parse_statement, SqlErrorKind};
use cassie::types::Value;

const PREFIX_INSERT: &str =
    "INSERT INTO prefix_dst(flag) WITH c AS(SELECT true AS flag) SELECT flag FROM c RETURNING flag";

#[test]
fn should_reject_unconsumed_insert_target_tail() {
    // Arrange
    let inputs = [
        PREFIX_INSERT,
        "INSERT INTO prefix_dst(flag) WITH c AS(SELECT true AS flag) SELECT * FROM c RETURNING flag",
        "INSERT INTO prefix_dst(flag) garbage SELECT flag FROM c",
        "INSERT INTO prefix_dst(flag) garbage VALUES (true)",
        "INSERT INTO prefix_dst(flag) /* separator */ garbage SELECT flag FROM c",
        "INSERT INTO prefix_dst(flag) 'garbage' SELECT flag FROM c",
        "INSERT INTO prefix_dst(flag) \"garbage\" VALUES (true)",
    ];

    // Act
    let parsed = inputs.map(parse_statement);

    // Assert
    for (sql, result) in inputs.into_iter().zip(parsed) {
        let error = result.expect_err(sql);
        assert_eq!(error.kind(), SqlErrorKind::Syntax);
        assert_eq!(error.message(), "INSERT column list is malformed");
    }
}

#[test]
fn should_reject_source_prefix_before_catalog_lookup_or_writes() {
    // Arrange
    let absent = crate::support_sql_fixture::sql_fixture(
        "insert-tail-absent",
        &["CREATE TABLE prefix_dst(flag BOOLEAN)"],
    );
    let shadow = crate::support_sql_fixture::sql_fixture(
        "insert-tail-shadow",
        &[
            "CREATE TABLE prefix_dst(flag BOOLEAN)",
            "CREATE TABLE c(flag BOOLEAN)",
            "INSERT INTO c VALUES(false)",
        ],
    );

    // Act
    let results = [&absent, &shadow].map(|fixture| {
        let result = fixture.execute(PREFIX_INSERT);
        let observer = fixture.cassie.create_session("observer", None);
        let rows = fixture
            .cassie
            .execute_sql(&observer, "SELECT flag FROM prefix_dst", vec![])
            .expect("fresh observer")
            .rows;
        (result, rows)
    });

    // Assert
    assert!(
        results.iter().all(|(result, _)| matches!(result, Err(CassieError::InvalidQuery(message)) if message == "INSERT column list is malformed")),
        "expected parser rejection before catalog lookup: {results:?}"
    );
    for (_, rows) in results {
        assert_eq!(rows, [] as [Vec<Value>; 0]);
    }
    assert_eq!(
        shadow.rows("SELECT flag FROM c"),
        vec![vec![Value::Bool(false)]]
    );
}

#[test]
fn should_preserve_insert_sources_across_lexical_separators() {
    // Arrange
    let forms = [
        ("prefix_dst", "flag", "VALUES (true)", " RETURNING flag"),
        (
            "prefix_dst",
            "flag",
            "SELECT flag FROM c",
            " RETURNING flag",
        ),
        (
            "\"SELECT.values\"",
            "\"flag value\"",
            "SELECT 'VALUES SELECT' AS \"flag value\"",
            " RETURNING \"flag value\"",
        ),
        (
            "prefix_dst",
            "flag",
            "VALUES (true)",
            " ON CONFLICT (flag) DO NOTHING RETURNING flag",
        ),
    ];
    let separators = [
        " ",
        "\t\n",
        "-- SELECT VALUES\n",
        "/* SELECT VALUES /* nested */ */",
        " /* comment */\t",
    ];

    // Act
    // Assert
    for (table, columns, source, suffix) in forms {
        let reference =
            parse_statement(&format!("INSERT INTO {table}({columns}) {source}{suffix}"))
                .expect("ordinary supported source");
        let reference = serde_json::to_value(reference.statement).expect("reference AST");
        for separator in separators {
            let sql = format!("INSERT INTO {table}({columns}){separator}{source}{suffix}");
            let parsed = parse_statement(&sql).expect("supported separators");
            assert_eq!(
                serde_json::to_value(parsed.statement).expect("AST"),
                reference,
                "{sql}"
            );
        }
    }
}

#[test]
fn should_reject_insert_source_prefix_on_pgwire_without_writing() {
    use crate::support_pgwire as wire;

    // Arrange
    let fixture = crate::support_sql_fixture::sql_fixture(
        "insert-tail-wire",
        &[
            "CREATE TABLE prefix_dst(flag BOOLEAN)",
            "CREATE TABLE c(flag BOOLEAN)",
            "INSERT INTO c VALUES(false)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (rejections, rows_before_valid, valid) = runtime.block_on(async {
        let server = wire::spawn_server(fixture.cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (reader, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(reader);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut rejections = Vec::new();
        for sql in [
            PREFIX_INSERT,
            "INSERT INTO prefix_dst(flag) WITH c AS(SELECT true AS flag) SELECT * FROM c RETURNING flag",
            "INSERT INTO prefix_dst(flag) garbage SELECT flag FROM c",
            "INSERT INTO prefix_dst(flag) garbage VALUES (true)",
        ] {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
            rejections.push(wire::read_frames_until_ready(&mut reader).await);
        }
        let observer = fixture.cassie.create_session("wire_observer", None);
        let rows = fixture.cassie.execute_sql(&observer, "SELECT flag FROM prefix_dst", vec![])
            .expect("fresh observer before valid command").rows;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(
            "INSERT INTO prefix_dst(flag)/* SELECT /* nested */ VALUES */VALUES (true) RETURNING flag"
        )]).await;
        let valid = wire::read_frames_until_ready(&mut reader).await;
        server.stop().await;
        (rejections, rows, valid)
    });
    let rows = fixture.rows("SELECT flag FROM prefix_dst");

    // Assert
    for frames in rejections {
        assert_eq!(
            wire::error_code(&frames).as_deref(),
            Some("42601"),
            "frames: {frames:?}; destination rows: {rows_before_valid:?}"
        );
        assert!(!frames.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(frames.last(), Some(&(b'Z', vec![b'I'])));
    }
    assert_eq!(rows_before_valid, [] as [Vec<Value>; 0]);
    assert_eq!(wire::error_code(&valid), None);
    assert_eq!(wire::row_description_names(&valid), vec!["flag"]);
    let description = wire::parse_row_description(
        &valid
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("description")
            .1,
    );
    assert_eq!(description.len(), 1);
    assert_eq!(
        (
            description[0].type_oid,
            description[0].type_size,
            description[0].type_mod,
            description[0].format_code
        ),
        (16, 1, -1, 0)
    );
    assert_eq!(wire::data_rows(&valid), vec![vec![Some("t".into())]]);
    assert!(valid
        .iter()
        .any(|(tag, payload)| *tag == b'C' && payload == b"INSERT 0 1\0"));
    assert_eq!(valid.last(), Some(&(b'Z', vec![b'I'])));
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}
