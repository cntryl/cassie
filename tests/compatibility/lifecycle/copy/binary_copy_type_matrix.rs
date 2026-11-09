use super::*;

async fn copy_out_frames(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await
}

fn copy_data(frames: &[(u8, Vec<u8>)]) -> Vec<u8> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'd')
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect()
}

fn copy_command(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

fn append_binary_copy_field(payload: &mut Vec<u8>, value: Option<&[u8]>) {
    match value {
        Some(value) => {
            payload.extend_from_slice(
                &i32::try_from(value.len())
                    .expect("COPY field size fits in a signed 32-bit length")
                    .to_be_bytes(),
            );
            payload.extend_from_slice(value);
        }
        None => payload.extend_from_slice(&(-1_i32).to_be_bytes()),
    }
}

fn scalar_binary_copy_payload() -> Vec<u8> {
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&9_i16.to_be_bytes());

    let smallint = (-2_i16).to_be_bytes();
    let bigint = 258_i64.to_be_bytes();
    let boolean = [1_u8];
    let float8 = 1.5_f64.to_be_bytes();
    let bytea = [0_u8, 255];
    let date = 1_i32.to_be_bytes();
    let timestamptz = 97_445_000_006_i64.to_be_bytes();
    let numeric = [0_u8, 2, 0, 0, 0, 0, 0, 2, 0, 12, 13, 72];

    for value in [
        smallint.as_slice(),
        bigint.as_slice(),
        boolean.as_slice(),
        float8.as_slice(),
        bytea.as_slice(),
        date.as_slice(),
        timestamptz.as_slice(),
        numeric.as_slice(),
        b"A".as_slice(),
    ] {
        append_binary_copy_field(&mut payload, Some(value));
    }
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    payload
}

fn binary_array_copy_input_payload() -> Vec<u8> {
    const INT4_ARRAY_WITH_NULL: [u8; 40] = [
        0, 0, 0, 1, // one dimension
        0, 0, 0, 1, // array contains a NULL element
        0, 0, 0, 23, // int4 element OID
        0, 0, 0, 3, // dimension length
        0, 0, 0, 1, // lower bound
        0, 0, 0, 4, // first element length
        0, 0, 0, 1, // first element value
        255, 255, 255, 255, // NULL element length
        0, 0, 0, 4, // third element length
        0, 0, 0, 3, // third element value
    ];
    const EMPTY_INT4_ARRAY: [u8; 12] = [
        0, 0, 0, 0, // zero dimensions
        0, 0, 0, 0, // no NULL elements
        0, 0, 0, 23, // int4 element OID
    ];

    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());

    // PostgreSQL 18.6 binary row: an SQL NULL text field, an int4 array with
    // one NULL element, and bytea containing both NUL and 0xff bytes.
    payload.extend_from_slice(&4_i16.to_be_bytes());
    let id = 42_i32.to_be_bytes();
    append_binary_copy_field(&mut payload, Some(&id));
    append_binary_copy_field(&mut payload, None);
    append_binary_copy_field(&mut payload, Some(&INT4_ARRAY_WITH_NULL));
    append_binary_copy_field(&mut payload, Some(&[0_u8, 255]));

    // The second row distinguishes SQL NULL array and bytea values from data.
    payload.extend_from_slice(&4_i16.to_be_bytes());
    let id = 43_i32.to_be_bytes();
    append_binary_copy_field(&mut payload, Some(&id));
    append_binary_copy_field(&mut payload, Some(b"text"));
    append_binary_copy_field(&mut payload, None);
    append_binary_copy_field(&mut payload, None);

    // The third row distinguishes empty text, an empty array, and empty bytea.
    payload.extend_from_slice(&4_i16.to_be_bytes());
    let id = 44_i32.to_be_bytes();
    append_binary_copy_field(&mut payload, Some(&id));
    append_binary_copy_field(&mut payload, Some(b""));
    append_binary_copy_field(&mut payload, Some(&EMPTY_INT4_ARRAY));
    append_binary_copy_field(&mut payload, Some(b""));
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    payload
}

#[test]
fn should_decode_pg_type_kind_as_internal_char() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_type_kind_internal_char").await;
        let (client, connection) = server.connect().await;
        let query = "SELECT t.typname, t.typtype, t.typelem, NULL::OID, \
                     t.typbasetype, n.nspname, t.typrelid \
                     FROM pg_catalog.pg_type t \
                     INNER JOIN pg_catalog.pg_namespace n \
                         ON t.typnamespace = n.oid \
                     WHERE t.oid = $1";

        // Act
        let statement = client
            .prepare(query)
            .await
            .expect("prepare the pg_type type-info query");
        assert_eq!(statement.columns()[1].type_().oid(), 18);
        let row = client
            .query_one(&statement, &[&23_u32])
            .await
            .expect("read pg_type type info");
        let kind = row.get::<_, i8>(1);
        let type_oid = statement.columns()[1].type_().oid();

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(type_oid, 18);
        assert_eq!(kind, b'b'.cast_signed());
    });
}

#[test]
fn should_describe_and_decode_pg_type_kind_and_category_as_internal_chars() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_type_kind_category_internal_char").await;
        let (client, connection) = server.connect().await;

        // Act
        let direct = client.prepare("SELECT t.typtype AS kind, t.typcategory AS category FROM pg_catalog.pg_type t WHERE t.oid = $1").await.expect("prepare direct and aliased columns");
        let unknown = client.query_one(&direct, &[&705_u32]).await.expect("unknown type");
        let array = client.query_one(&direct, &[&1007_u32]).await.expect("array type");
        let wildcard = client.prepare("SELECT * FROM pg_catalog.pg_type WHERE oid = $1").await.expect("prepare wildcard");
        let wildcard_row = client.query_one(&wildcard, &[&1007_u32]).await.expect("array wildcard");
        let kind = wildcard.columns().iter().position(|column| column.name() == "typtype").expect("kind column");
        let category = wildcard.columns().iter().position(|column| column.name() == "typcategory").expect("category column");

        // Assert
        assert_eq!(direct.columns()[0].type_().oid(), 18);
        assert_eq!(direct.columns()[1].type_().oid(), 18);
        assert_eq!((unknown.get::<_, i8>(0), unknown.get::<_, i8>(1)), (b'p'.cast_signed(), b'X'.cast_signed()));
        assert_eq!((array.get::<_, i8>(0), array.get::<_, i8>(1)), (b'b'.cast_signed(), b'A'.cast_signed()));
        assert_eq!(wildcard.columns()[kind].type_().oid(), 18);
        assert_eq!(wildcard.columns()[category].type_().oid(), 18);
        assert_eq!((wildcard_row.get::<_, i8>(kind), wildcard_row.get::<_, i8>(category)), (b'b'.cast_signed(), b'A'.cast_signed()));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_unrelated_joined_typcategory_as_text() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_type_unrelated_category").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE other_category (typcategory TEXT)")
            .await
            .expect("create unrelated column");
        client
            .batch_execute("INSERT INTO other_category VALUES ('long')")
            .await
            .expect("insert unrelated value");

        // Act
        let statement = client.prepare("SELECT o.typcategory FROM pg_catalog.pg_type t JOIN other_category o ON t.oid = 23").await.expect("prepare join");
        let row = client.query_one(&statement, &[]).await.expect("read unrelated value");

        // Assert
        assert_eq!(statement.columns()[0].type_().oid(), 25);
        assert_eq!(row.get::<_, &str>(0), "long");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_wildcard_duplicate_alias_as_text() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("pg_type_wildcard_category_alias").await;
        let (client, connection) = server.connect().await;

        // Act
        let statement = client
            .prepare("SELECT *, 'long' AS typcategory FROM pg_catalog.pg_type WHERE oid = 23")
            .await
            .expect("prepare wildcard and duplicate alias");
        let row = client
            .query_one(&statement, &[])
            .await
            .expect("read wildcard and alias");
        let alias = statement
            .columns()
            .iter()
            .rposition(|column| column.name() == "typcategory")
            .expect("duplicate alias");

        // Assert
        assert_eq!(statement.columns()[alias].type_().oid(), 25);
        assert_eq!(row.get::<_, &str>(alias), "long");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_export_binary_copy_rows_with_postgresql_scalar_type_encodings() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_scalar_type_matrix").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_binary_scalar_matrix (
                     small_value SMALLINT,
                     big_value BIGINT,
                     enabled BOOLEAN,
                     ratio DOUBLE PRECISION,
                     payload BYTEA,
                     day DATE,
                     moment TIMESTAMPTZ,
                     amount NUMERIC,
                     label TEXT
                 )",
            )
            .await
            .expect("create the binary COPY scalar matrix");
        client
            .batch_execute(
                "INSERT INTO compat_copy_binary_scalar_matrix VALUES (
                     -2, 258, TRUE, 1.5, decode('00ff', 'hex'),
                     DATE '2000-01-02',
                     TIMESTAMPTZ '2000-01-02 03:04:05.000006+00',
                     12.34, 'A'
                 )",
            )
            .await
            .expect("populate the binary COPY scalar matrix");

        // Act
        let frames = copy_out_frames(
            &server,
            "COPY compat_copy_binary_scalar_matrix TO STDOUT WITH (FORMAT BINARY)",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let response = frames
            .iter()
            .find(|(tag, _)| *tag == b'H')
            .map(|(_, payload)| payload.clone());
        let command = copy_command(&frames);
        let completed = frames.iter().any(|(tag, _)| *tag == b'c');

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let mut expected_response = vec![1, 0, 9];
        for _ in 0..9 {
            expected_response.extend_from_slice(&1_i16.to_be_bytes());
        }
        assert_eq!(error_code, None, "binary COPY SQLSTATE; frames: {frames:?}");
        assert_eq!(response, Some(expected_response));
        assert_eq!(copy_data(&frames), scalar_binary_copy_payload());
        assert_eq!(command.as_deref(), Some("COPY 1"));
        assert!(completed, "binary COPY TO should send CopyDone");
    });
}

#[test]
fn should_import_binary_copy_null_empty_array_payloads() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_array_null_input_matrix").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_binary_input_matrix (
                     id INT4,
                     note TEXT,
                     nums INT4[],
                     payload BYTEA
                 )",
            )
            .await
            .expect("create the binary COPY input target");

        // Act
        let (response_tag, response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_binary_input_matrix FROM STDIN WITH (FORMAT BINARY)",
            &binary_array_copy_input_payload(),
        )
        .await;
        let descriptor = client
            .prepare("SELECT id, note, nums, payload FROM compat_copy_binary_input_matrix")
            .await
            .expect("describe the binary COPY target columns");
        let type_oids = descriptor
            .columns()
            .iter()
            .map(|column| column.type_().oid())
            .collect::<Vec<_>>();
        let rows = client
            .query(
                "SELECT id, note, nums, nums::text, payload::text \
                 FROM compat_copy_binary_input_matrix ORDER BY id",
                &[],
            )
            .await
            .expect("read rows imported through binary COPY");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "valid binary COPY should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert_eq!(type_oids, vec![23, 25, 1007, 17]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 42);
        assert_eq!(rows[0].get::<_, Option<String>>(1), None);
        assert_eq!(
            rows[0].get::<_, Option<Vec<Option<i32>>>>(2),
            Some(vec![Some(1), None, Some(3)])
        );
        assert_eq!(
            rows[0].get::<_, Option<String>>(3).as_deref(),
            Some("{1,NULL,3}")
        );
        assert_eq!(
            rows[0].get::<_, Option<String>>(4).as_deref(),
            Some("\\x00ff")
        );
        assert_eq!(rows[1].get::<_, i32>(0), 43);
        assert_eq!(rows[1].get::<_, Option<String>>(1).as_deref(), Some("text"));
        assert_eq!(rows[1].get::<_, Option<Vec<Option<i32>>>>(2), None);
        assert_eq!(rows[1].get::<_, Option<String>>(3), None);
        assert_eq!(rows[1].get::<_, Option<String>>(4), None);
        assert_eq!(rows[2].get::<_, i32>(0), 44);
        assert_eq!(rows[2].get::<_, Option<String>>(1).as_deref(), Some(""));
        assert_eq!(rows[2].get::<_, Option<Vec<Option<i32>>>>(2), Some(vec![]));
        assert_eq!(rows[2].get::<_, Option<String>>(3).as_deref(), Some("{}"));
        assert_eq!(rows[2].get::<_, Option<String>>(4).as_deref(), Some("\\x"));
    });
}

#[test]
fn should_reject_binary_copy_with_invalid_signature_atomically() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_binary_invalid_signature").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_copy_binary_invalid_signature (id INT4 PRIMARY KEY)",
            )
            .await
            .expect("create the binary COPY target");
        client
            .batch_execute("INSERT INTO compat_copy_binary_invalid_signature VALUES (99)")
            .await
            .expect("seed the binary COPY target");

        // Act
        let (response_tag, response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_binary_invalid_signature FROM STDIN \
             WITH (FORMAT BINARY)",
            b"not the PostgreSQL binary COPY signature",
        )
        .await;
        let rows = client
            .query(
                "SELECT id FROM compat_copy_binary_invalid_signature ORDER BY id",
                &[],
            )
            .await
            .expect("read the target after binary COPY rejects its signature");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            response_tag, b'G',
            "valid binary COPY syntax should enter COPY IN; response: {response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(error_code.as_deref(), Some("22P04"));
        assert_eq!(command, None);
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![99],
            "invalid binary input must not change rows that predate COPY"
        );
    });
}
