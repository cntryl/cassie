#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;

#[path = "support/pgwire.rs"]
mod wire;
use wire::*;

const OID_BOOL: i32 = 16;
const OID_INT4: i32 = 23;
const OID_TEXT: i32 = 25;
const OID_UNKNOWN: i32 = 705;

#[test]
fn should_describe_typed_insert_returning_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("typed_insert_returning");
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
                "CREATE TABLE pgwire_typed_items (id TEXT, score INT, active BOOLEAN)",
                vec![],
            )
            .unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame_with_types(
                    "typed_insert",
                    "INSERT INTO pgwire_typed_items (id, score, active) VALUES ($1, $2, $3) RETURNING id, score, active",
                    &[OID_TEXT, OID_INT4, OID_BOOL],
                ),
                describe_statement_frame("typed_insert"),
                bind_frame("typed_insert_portal", "typed_insert", &["typed-1", "42", "true"]),
                execute_frame("typed_insert_portal"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b't', b'T', b'2', b'D', b'C', b'Z']
        );
        assert_eq!(
            parse_parameter_description(&frames[1].1),
            vec![OID_TEXT, OID_INT4, OID_BOOL]
        );
        let columns = parse_row_description(&frames[2].1);
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.name.as_str(), column.type_oid))
                .collect::<Vec<_>>(),
            vec![("id", OID_TEXT), ("score", OID_INT4), ("active", OID_BOOL)]
        );
        assert_eq!(
            parse_data_row(&frames[4].1),
            vec![
                Some("typed-1".to_string()),
                Some("42".to_string()),
                Some("true".to_string())
            ]
        );
        assert_eq!(frames[6].1, vec![b'I']);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_infer_parameter_metadata_for_crud_returning_flows() {
    // Arrange
    with_fallback();
    let path = data_dir("infer_crud_metadata");
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
                "CREATE TABLE pgwire_infer_items (label TEXT, score INT, active BOOLEAN)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO pgwire_infer_items (label, score, active) VALUES ('item-1', 7, true)",
                vec![],
            )
            .unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame(
                    "infer_insert",
                    "INSERT INTO pgwire_infer_items (label) VALUES ($1) RETURNING label",
                ),
                describe_statement_frame("infer_insert"),
                parse_frame(
                    "infer_select",
                    "SELECT label FROM pgwire_infer_items WHERE score = $1 AND active = $2",
                ),
                describe_statement_frame("infer_select"),
                parse_frame(
                    "infer_update",
                    "UPDATE pgwire_infer_items SET score = $1 WHERE label = $2 RETURNING score",
                ),
                describe_statement_frame("infer_update"),
                parse_frame(
                    "infer_delete",
                    "DELETE FROM pgwire_infer_items WHERE label = $1 RETURNING label",
                ),
                describe_statement_frame("infer_delete"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b't', b'T', b'1', b't', b'T', b'1', b't', b'T', b'1', b't', b'T', b'Z']
        );
        assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_TEXT]);
        assert_eq!(
            parse_parameter_description(&frames[4].1),
            vec![OID_INT4, OID_BOOL]
        );
        assert_eq!(
            parse_parameter_description(&frames[7].1),
            vec![OID_INT4, OID_TEXT]
        );
        assert_eq!(parse_parameter_description(&frames[10].1), vec![OID_TEXT]);
        assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_TEXT);
        assert_eq!(parse_row_description(&frames[5].1)[0].type_oid, OID_TEXT);
        assert_eq!(parse_row_description(&frames[8].1)[0].type_oid, OID_INT4);
        assert_eq!(parse_row_description(&frames[11].1)[0].type_oid, OID_TEXT);
        assert_eq!(frames[12].1, vec![b'I']);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reuse_unnamed_statement_metadata_lifecycle() {
    // Arrange
    with_fallback();
    let path = data_dir("unnamed_metadata");
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
                "CREATE TABLE pgwire_unnamed_items (label TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO pgwire_unnamed_items (label, score) VALUES ('item-1', 5)",
                vec![],
            )
            .unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame(
                    "",
                    "SELECT label FROM pgwire_unnamed_items WHERE score = $1 ORDER BY label",
                ),
                describe_statement_frame(""),
                bind_frame("", "", &["5"]),
                describe_portal_frame(""),
                execute_frame(""),
                close_portal_frame(""),
                close_statement_frame(""),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b't', b'T', b'2', b'T', b'D', b'C', b'3', b'3', b'Z']
        );
        assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_INT4]);
        assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_TEXT);
        assert_eq!(parse_row_description(&frames[4].1)[0].type_oid, OID_TEXT);
        assert_eq!(
            parse_data_row(&frames[5].1),
            vec![Some("item-1".to_string())]
        );
        assert_eq!(frames[9].1, vec![b'I']);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_table_free_parameter_oids_through_describe() {
    // Arrange
    with_fallback();
    let path = data_dir("table_free_parameter_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame("table_free_inferred", "SELECT $1::INT AS value"),
                describe_statement_frame("table_free_inferred"),
                parse_frame_with_types("table_free_explicit", "SELECT $1 AS value", &[OID_BOOL]),
                describe_statement_frame("table_free_explicit"),
                sync_frame(),
            ],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b't', b'T', b'1', b't', b'T', b'Z']
        );
        assert_eq!(parse_parameter_description(&frames[1].1), vec![OID_INT4]);
        assert_eq!(parse_row_description(&frames[2].1)[0].type_oid, OID_INT4);
        assert_eq!(parse_parameter_description(&frames[4].1), vec![OID_BOOL]);
        assert_eq!(parse_row_description(&frames[5].1)[0].type_oid, OID_BOOL);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_recover_ready_state_after_extended_statement_error() {
    // Arrange
    with_fallback();
    let path = data_dir("extended_error_recovery");
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
                "CREATE TABLE pgwire_recovery_items (label TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO pgwire_recovery_items (label, score) VALUES ('item-1', 9)",
                vec![],
            )
            .unwrap();
        let server = spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        complete_startup(&mut reader, &mut write_half).await;

        // Act
        write_frames(
            &mut write_half,
            vec![
                parse_frame(
                    "recovery_stmt",
                    "SELECT label FROM pgwire_recovery_items WHERE score = $1",
                ),
                bind_frame("bad_portal", "recovery_stmt", &["9", "extra"]),
                parse_frame(
                    "ignored_stmt",
                    "SELECT label FROM pgwire_recovery_items WHERE score = $1",
                ),
                bind_frame("ignored_portal", "ignored_stmt", &["9"]),
                execute_frame("ignored_portal"),
                sync_frame(),
            ],
        )
        .await;
        let error_frames = read_frames_until_ready(&mut reader).await;
        write_frames(
            &mut write_half,
            vec![
                bind_frame("good_portal", "recovery_stmt", &["9"]),
                execute_frame("good_portal"),
                sync_frame(),
            ],
        )
        .await;
        let recovery_frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            error_frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'1', b'E', b'Z']
        );
        let fields = parse_error_fields(&error_frames[1].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'S' && value == "ERROR"));
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'C' && value == "08P01"));
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value.contains("requires 1")));
        assert_eq!(error_frames[2].1, vec![b'I']);

        assert_eq!(
            recovery_frames
                .iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            vec![b'2', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(
            parse_data_row(&recovery_frames[2].1),
            vec![Some("item-1".to_string())]
        );
        assert_eq!(recovery_frames[4].1, vec![b'I']);

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}
