use super::*;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

fn frontend_wire_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("frontend frame length must fit in i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn startup_wire_frame() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0root\0database\0postgres\0\0");

    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup frame length must fit in i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_wire_frame(reader: &mut (impl AsyncRead + Unpin)) -> (u8, Vec<u8>) {
    let mut tag = [0_u8; 1];
    reader.read_exact(&mut tag).await.expect("read frame tag");
    let mut length = [0_u8; 4];
    reader
        .read_exact(&mut length)
        .await
        .expect("read frame length");
    let length = i32::from_be_bytes(length);
    let payload_length = usize::try_from(length - 4).expect("valid frame payload length");
    let mut payload = vec![0_u8; payload_length];
    reader
        .read_exact(&mut payload)
        .await
        .expect("read frame payload");
    (tag[0], payload)
}

async fn read_frames_until_ready(reader: &mut (impl AsyncRead + Unpin)) -> Vec<(u8, Vec<u8>)> {
    let mut frames = Vec::new();
    loop {
        let frame = read_wire_frame(reader).await;
        let ready = frame.0 == b'Z';
        frames.push(frame);
        if ready {
            return frames;
        }
    }
}

async fn connect_fastpath_client(server: &CompatibilityServer) -> tokio::net::TcpStream {
    let mut socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw large-object fastpath client");
    socket
        .write_all(&startup_wire_frame())
        .await
        .expect("send startup packet");
    let (auth_tag, auth_payload) = read_wire_frame(&mut socket).await;
    assert_eq!(auth_tag, b'R', "startup returns an authentication response");
    let auth_code = i32::from_be_bytes(
        auth_payload[..4]
            .try_into()
            .expect("authentication response code"),
    );
    if auth_code == 3 {
        socket
            .write_all(&frontend_wire_frame(b'p', b"postgres\0"))
            .await
            .expect("send the configured password");
        let (auth_ok_tag, auth_ok_payload) = read_wire_frame(&mut socket).await;
        assert_eq!(auth_ok_tag, b'R', "password success is an auth response");
        assert_eq!(
            i32::from_be_bytes(
                auth_ok_payload[..4]
                    .try_into()
                    .expect("authentication success code")
            ),
            0,
            "the configured password should authenticate the raw client"
        );
    } else {
        assert_eq!(
            auth_code, 0,
            "the server accepts only completed authentication"
        );
    }
    let startup_frames = read_frames_until_ready(&mut socket).await;
    assert!(
        startup_frames.iter().all(|(tag, _)| *tag != b'E'),
        "raw fastpath client startup should succeed: {startup_frames:?}"
    );
    socket
}

fn error_sqlstate(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
    let fields = payload.split(|byte| *byte == 0).collect::<Vec<_>>();
    fields
        .windows(2)
        .find(|pair| pair[0] == b"C")
        .map(|pair| String::from_utf8_lossy(pair[1]).into_owned())
}

fn function_call_response_oid(payload: &[u8]) -> Option<u32> {
    let result_length = i32::from_be_bytes(payload.get(..4)?.try_into().ok()?);
    if result_length != 4 || payload.len() != 8 {
        return None;
    }
    Some(u32::from_be_bytes(payload[4..8].try_into().ok()?))
}

fn large_object_wire_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

#[test]
fn should_preserve_postgresql_wire_oids_for_large_object_io() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_wire_oids").await;
        let (client, connection) = server.connect().await;
        let created = client
            .query_one("SELECT lo_from_bytea(0, decode('6162', 'hex'))", &[])
            .await
            .expect("create a large object with a known byte sequence");
        let object_id: u32 = created.try_get(0).expect("large-object OID value");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin large-object descriptor transaction");
        let descriptor = client
            .query_one(&format!("SELECT lo_open({object_id}::oid, 393216)"), &[])
            .await
            .expect("open the large object for reading and writing");
        let descriptor_id: i32 = descriptor.try_get(0).expect("descriptor value");

        // Act
        let read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect("read one byte from the descriptor");
        let read_bytes: Vec<u8> = read.try_get(0).expect("large-object read bytes");
        let written = client
            .query_one(
                &format!("SELECT lowrite({descriptor_id}, decode('ff', 'hex'))"),
                &[],
            )
            .await
            .expect("write one byte at the current descriptor position");
        let seek = client
            .query_one(
                &format!("SELECT lo_lseek64({descriptor_id}, 0::bigint, 0)"),
                &[],
            )
            .await
            .expect("seek to the beginning with the 64-bit function");
        let position: i64 = client
            .query_one(&format!("SELECT lo_tell64({descriptor_id})"), &[])
            .await
            .expect("read the 64-bit descriptor position")
            .try_get(0)
            .expect("64-bit position");
        let truncated = client
            .query_one(
                &format!("SELECT lo_truncate64({descriptor_id}, 3::bigint)"),
                &[],
            )
            .await
            .expect("extend the object with the 64-bit truncate function");
        let closed = client
            .query_one(&format!("SELECT lo_close({descriptor_id})"), &[])
            .await
            .expect("close the large-object descriptor");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit large-object descriptor transaction");
        let bytes: Vec<u8> = client
            .query_one(&format!("SELECT lo_get({object_id}::oid)"), &[])
            .await
            .expect("read the final object bytes")
            .try_get(0)
            .expect("final object bytea");

        // Assert
        // PostgreSQL 18.6 wire OIDs: oid=26, int4=23, bytea=17, int8=20.
        assert_eq!(created.columns()[0].type_().oid(), 26);
        assert_eq!(descriptor.columns()[0].type_().oid(), 23);
        assert_eq!(read.columns()[0].type_().oid(), 17);
        assert_eq!(written.columns()[0].type_().oid(), 23);
        assert_eq!(seek.columns()[0].type_().oid(), 20);
        assert_eq!(seek.try_get::<_, i64>(0).expect("seek result"), 0);
        assert_eq!(position, 0);
        assert_eq!(truncated.columns()[0].type_().oid(), 23);
        assert_eq!(truncated.try_get::<_, i32>(0).expect("truncate result"), 0);
        assert_eq!(closed.columns()[0].type_().oid(), 23);
        assert_eq!(closed.try_get::<_, i32>(0).expect("close result"), 0);
        assert_eq!(read_bytes, [0x61]);
        assert_eq!(written.try_get::<_, i32>(0).expect("write result"), 1);
        assert_eq!(bytes, [0x61, 0xff, 0x00]);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_missing_large_object_open_with_postgresql_sqlstate() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_missing_open_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let missing = client
            .query_one("SELECT lo_open(4294967295::oid, 262144)", &[])
            .await
            .expect_err("opening a missing large object must fail");

        // Assert
        // PostgreSQL 18.6 returns 42704 (undefined_object) for a missing LO OID.
        assert_eq!(db_error(&missing).code().code(), "42704");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_large_object_with_a_postgresql_oid_through_lo_create() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_create_oid").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .query_one("SELECT lo_create(0)", &[])
            .await
            .expect("PostgreSQL 18.6 creates an empty large object and returns its OID");
        let object_id: u32 = created.try_get(0).expect("created large-object OID");
        let removed = client
            .query_one(&format!("SELECT lo_unlink({object_id}::oid)"), &[])
            .await
            .expect("remove the empty large-object fixture");

        // Assert
        assert_eq!(created.columns()[0].type_().oid(), 26);
        assert_ne!(object_id, 0);
        assert_eq!(removed.get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_large_object_through_postgresql_fastpath_function_call() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_fastpath_function_call").await;
        let (client, connection) = server.connect().await;
        let mut fastpath = connect_fastpath_client(&server).await;
        fastpath
            .write_all(&frontend_wire_frame(b'Q', b"BEGIN\0"))
            .await
            .expect("begin a transaction for the fastpath call");
        let begin_frames = read_frames_until_ready(&mut fastpath).await;

        // Act
        // PostgreSQL 18.6 assigns lo_creat(int4) OID 957 and returns OID (26).
        let mut payload = Vec::new();
        payload.extend_from_slice(&957_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        payload.extend_from_slice(&4_i32.to_be_bytes());
        payload.extend_from_slice(&0_i32.to_be_bytes());
        payload.extend_from_slice(&1_i16.to_be_bytes());
        fastpath
            .write_all(&frontend_wire_frame(b'F', &payload))
            .await
            .expect("send binary lo_creat fastpath call");
        let function_response = tokio::time::timeout(
            Duration::from_secs(1),
            read_frames_until_ready(&mut fastpath),
        )
        .await;
        let function_timed_out = function_response.is_err();
        let function_frames = function_response.unwrap_or_default();
        let function_oid = function_frames
            .iter()
            .find(|(tag, _)| *tag == b'V')
            .and_then(|(_, payload)| function_call_response_oid(payload));
        let function_sqlstate = error_sqlstate(&function_frames);

        let rollback_frames = if function_timed_out {
            // A canceled frame read may have consumed part of a backend frame, so do not
            // reuse this connection's reader. Queue ROLLBACK when possible, then close the
            // session; closing it also rolls back any transaction still in progress.
            let rollback = frontend_wire_frame(b'Q', b"ROLLBACK\0");
            let _ = tokio::time::timeout(
                Duration::from_millis(100),
                fastpath.write_all(&rollback),
            )
            .await;
            let _ = tokio::time::timeout(Duration::from_millis(100), fastpath.shutdown()).await;
            None
        } else {
            fastpath
                .write_all(&frontend_wire_frame(b'Q', b"ROLLBACK\0"))
                .await
                .expect("roll back the fastpath-created object");
            Some(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    read_frames_until_ready(&mut fastpath),
                )
                .await
                .expect("ROLLBACK should reach ReadyForQuery"),
            )
        };
        let rolled_back_object_count = if let Some(object_oid) = function_oid {
            Some(
                client
                    .query_one(
                        &format!(
                            "SELECT count(*) FROM pg_catalog.pg_largeobject_metadata \
                             WHERE oid = {object_oid}::oid"
                        ),
                        &[],
                    )
                    .await
                    .expect("inspect the object after rolling back its creation")
                    .get::<_, i64>(0),
            )
        } else {
            None
        };

        // Assert
        drop((fastpath, client));
        server.shutdown(connection).await;
        assert!(
            !function_timed_out,
            "PG18.6 lo_creat fastpath should return a FunctionCallResponse before the bounded timeout; the response timed out without SQLSTATE evidence, and the client session was closed to roll back"
        );
        assert_eq!(error_sqlstate(&begin_frames), None);
        assert_eq!(
            begin_frames.last().map(|(_, payload)| payload.as_slice()),
            Some(&b"T"[..])
        );
        assert_eq!(
            function_sqlstate, None,
            "PG18.6 lo_creat fastpath should return a value; frames: {function_frames:?}"
        );
        assert!(
            function_frames.iter().any(|(tag, _)| *tag == b'V'),
            "PG18.6 lo_creat fastpath returns FunctionCallResponse: {function_frames:?}"
        );
        assert!(function_oid.is_some_and(|oid| oid != 0));
        let rollback_frames = rollback_frames.expect("completed fastpath call keeps its session");
        assert_eq!(error_sqlstate(&rollback_frames), None);
        assert_eq!(
            rollback_frames
                .last()
                .map(|(_, payload)| payload.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(rolled_back_object_count, Some(0));
    });
}

#[test]
fn should_invalidate_large_object_descriptor_at_transaction_commit() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_descriptor_commit_lifetime").await;
        let (client, connection) = server.connect().await;
        let created = client
            .query_one("SELECT lo_from_bytea(0, decode('6162', 'hex'))", &[])
            .await
            .expect("create a large object with known bytes");
        let object_id: u32 = created.try_get(0).expect("large-object OID");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin descriptor-lifetime transaction");
        let opened = client
            .query_one(&format!("SELECT lo_open({object_id}::oid, 262144)"), &[])
            .await
            .expect("open the object for reading inside the transaction");
        let descriptor_id: i32 = opened.try_get(0).expect("large-object descriptor");
        let in_transaction_read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect("read through the descriptor before its transaction ends");
        let in_transaction_bytes: Vec<u8> = in_transaction_read
            .try_get(0)
            .expect("in-transaction large-object bytes");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit the descriptor-lifetime transaction");

        // Act
        let expired_read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect_err("transaction completion must close the descriptor");
        let persisted = client
            .query_one(&format!("SELECT lo_get({object_id}::oid)"), &[])
            .await
            .expect("read the object after the descriptor expires");
        let persisted_bytes: Vec<u8> = persisted
            .try_get(0)
            .expect("large-object bytes survive descriptor closure");
        let unlinked = client
            .query_one(&format!("SELECT lo_unlink({object_id}::oid)"), &[])
            .await
            .expect("remove the fixture large object");

        // Assert
        // PostgreSQL 18.6 reports 42704 for use of a descriptor after commit.
        assert_eq!(in_transaction_bytes, [0x61]);
        assert_eq!(db_error(&expired_read).code().code(), "42704");
        assert_eq!(persisted_bytes, [0x61, 0x62]);
        assert_eq!(unlinked.try_get::<_, i32>(0).expect("unlink status"), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
