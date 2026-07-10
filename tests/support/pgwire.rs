#![allow(dead_code)]

use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

pub struct PgwireServer {
    pub addr: SocketAddr,
    handle: tokio::task::JoinHandle<Result<(), CassieError>>,
}

impl PgwireServer {
    pub async fn stop(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowDescription {
    pub name: String,
    pub table_oid: i32,
    pub attr_num: i16,
    pub type_oid: i32,
    pub type_size: i16,
    pub type_mod: i32,
    pub format_code: i16,
}

pub fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

pub fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-pgwire-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

pub async fn spawn_server(cassie: Cassie) -> PgwireServer {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password.clear();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);

    let handle = tokio::spawn(cassie::pgwire::server::run(
        addr.to_string(),
        std::sync::Arc::new(cassie),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;

    PgwireServer { addr, handle }
}

pub fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

pub fn password_message(password: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(password.as_bytes());
    payload.push(0);

    frontend_frame(b'p', &payload)
}

pub fn parse_frame(statement_name: &str, sql: &str) -> Vec<u8> {
    parse_frame_with_types(statement_name, sql, &[])
}

pub fn parse_frame_with_types(statement_name: &str, sql: &str, parameter_types: &[i32]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(sql.as_bytes());
    payload.push(0);
    payload.extend_from_slice(
        &i16::try_from(parameter_types.len())
            .expect("parse parameter count must fit into i16")
            .to_be_bytes(),
    );
    for oid in parameter_types {
        payload.extend_from_slice(&oid.to_be_bytes());
    }

    frontend_frame(b'P', &payload)
}

pub fn bind_frame(portal_name: &str, statement_name: &str, params: &[&str]) -> Vec<u8> {
    let params = params
        .iter()
        .map(|param| Some(param.as_bytes()))
        .collect::<Vec<_>>();
    bind_frame_with_formats(portal_name, statement_name, &[0], &params, &[])
}

pub fn bind_frame_with_formats(
    portal_name: &str,
    statement_name: &str,
    parameter_formats: &[i16],
    params: &[Option<&[u8]>],
    result_formats: &[i16],
) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(portal_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(
        &i16::try_from(parameter_formats.len())
            .expect("parameter format count must fit into i16")
            .to_be_bytes(),
    );
    for format in parameter_formats {
        payload.extend_from_slice(&format.to_be_bytes());
    }
    payload.extend_from_slice(
        &i16::try_from(params.len())
            .expect("parameter count must fit into i16")
            .to_be_bytes(),
    );
    for param in params {
        match param {
            Some(param) => {
                payload.extend_from_slice(
                    &i32::try_from(param.len())
                        .expect("parameter length must fit into i32")
                        .to_be_bytes(),
                );
                payload.extend_from_slice(param);
            }
            None => payload.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    payload.extend_from_slice(
        &i16::try_from(result_formats.len())
            .expect("result format count must fit into i16")
            .to_be_bytes(),
    );
    for format in result_formats {
        payload.extend_from_slice(&format.to_be_bytes());
    }

    frontend_frame(b'B', &payload)
}

pub fn describe_statement_frame(statement_name: &str) -> Vec<u8> {
    describe_frame(b'S', statement_name)
}

pub fn describe_portal_frame(portal_name: &str) -> Vec<u8> {
    describe_frame(b'P', portal_name)
}

pub fn execute_frame(portal_name: &str) -> Vec<u8> {
    execute_limited_frame(portal_name, 0)
}

pub fn execute_limited_frame(portal_name: &str, limit: i32) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(portal_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&limit.to_be_bytes());
    frontend_frame(b'E', &payload)
}

pub fn close_statement_frame(statement_name: &str) -> Vec<u8> {
    close_frame(b'S', statement_name)
}

pub fn close_portal_frame(portal_name: &str) -> Vec<u8> {
    close_frame(b'P', portal_name)
}

pub fn sync_frame() -> Vec<u8> {
    frontend_frame(b'S', &[])
}

pub async fn complete_startup(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
) {
    writer
        .write_all(&startup_frame("postgres", "postgres"))
        .await
        .expect("write startup");
    let auth = read_wire_frame(reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    if auth_request_code(&auth.1) == Some(3) {
        writer
            .write_all(&password_message("postgres"))
            .await
            .expect("write password");
        writer.flush().await.expect("flush password");
        let auth_ok = read_wire_frame(reader).await;
        assert_eq!(auth_ok.0, b'R', "password auth should return auth response");
        assert_eq!(
            auth_request_code(&auth_ok.1),
            Some(0),
            "password auth should complete with auth ok"
        );
    }
    let ready = read_until_ready(reader).await;
    assert_eq!(ready, vec![b'I']);
}

pub async fn write_frames(writer: &mut (impl AsyncWrite + Unpin), frames: Vec<Vec<u8>>) {
    for frame in frames {
        writer.write_all(&frame).await.expect("write frame");
    }
    writer.flush().await.expect("flush frames");
}

pub async fn read_wire_frame(reader: &mut (impl AsyncRead + Unpin)) -> (u8, Vec<u8>) {
    let mut tag = [0u8; 1];
    reader.read_exact(&mut tag).await.expect("read frame tag");

    let mut len = [0u8; 4];
    reader
        .read_exact(&mut len)
        .await
        .expect("read frame length");
    let len = i32::from_be_bytes(len);
    let mut payload = vec![0u8; usize::try_from(len - 4).expect("non-negative payload length")];
    if !payload.is_empty() {
        reader
            .read_exact(&mut payload)
            .await
            .expect("read frame payload");
    }

    (tag[0], payload)
}

pub async fn read_until_ready(reader: &mut (impl AsyncRead + Unpin)) -> Vec<u8> {
    loop {
        let frame = read_wire_frame(reader).await;
        if frame.0 == b'Z' {
            return frame.1;
        }
    }
}

pub async fn read_frames_until_ready(reader: &mut (impl AsyncRead + Unpin)) -> Vec<(u8, Vec<u8>)> {
    let mut frames = Vec::new();
    loop {
        let frame = read_wire_frame(reader).await;
        let tag = frame.0;
        frames.push(frame);
        if tag == b'Z' {
            return frames;
        }
    }
}

pub fn parse_row_description(payload: &[u8]) -> Vec<RowDescription> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut fields = Vec::new();

    for _ in 0..field_count {
        let name = read_cstring(payload, &mut cursor);
        let table_oid = read_i32(payload, &mut cursor);
        let attr_num = read_i16(payload, &mut cursor);
        let type_oid = read_i32(payload, &mut cursor);
        let type_size = read_i16(payload, &mut cursor);
        let type_mod = read_i32(payload, &mut cursor);
        let format_code = read_i16(payload, &mut cursor);
        fields.push(RowDescription {
            name,
            table_oid,
            attr_num,
            type_oid,
            type_size,
            type_mod,
            format_code,
        });
    }

    fields
}

fn auth_request_code(payload: &[u8]) -> Option<i32> {
    payload
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i32::from_be_bytes)
}

pub fn parse_parameter_description(payload: &[u8]) -> Vec<i32> {
    let mut cursor = 0usize;
    let parameter_count = read_i16(payload, &mut cursor);
    let mut parameters = Vec::new();

    for _ in 0..parameter_count {
        parameters.push(read_i32(payload, &mut cursor));
    }

    parameters
}

pub fn parse_data_row(payload: &[u8]) -> Vec<Option<String>> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut values = Vec::new();

    for _ in 0..field_count {
        let len = read_i32(payload, &mut cursor);
        if len < 0 {
            values.push(None);
            continue;
        }
        let len = usize::try_from(len).expect("payload length should fit usize");
        let end = cursor + len;
        let text = std::str::from_utf8(&payload[cursor..end]).expect("data row should be utf-8");
        cursor = end;
        values.push(Some(text.to_string()));
    }

    values
}

pub fn parse_error_fields(payload: &[u8]) -> Vec<(char, String)> {
    let mut cursor = 0usize;
    let mut fields = Vec::new();

    while cursor < payload.len() {
        let field_type = payload[cursor];
        cursor += 1;
        if field_type == 0 {
            break;
        }
        let value = read_cstring(payload, &mut cursor);
        fields.push((char::from(field_type), value));
    }

    fields
}

fn frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("frontend payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn describe_frame(target: u8, name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(target);
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    frontend_frame(b'D', &payload)
}

fn close_frame(target: u8, name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(target);
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);
    frontend_frame(b'C', &payload)
}

fn read_cstring(payload: &[u8], cursor: &mut usize) -> String {
    let tail = payload
        .get(*cursor..)
        .expect("cursor should be inside payload");
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring should be null terminated");
    let value = std::str::from_utf8(&tail[..end]).expect("cstring should be utf-8");
    *cursor += end + 1;
    value.to_string()
}

fn read_i16(payload: &[u8], cursor: &mut usize) -> i16 {
    let start = *cursor;
    let end = start + 2;
    let bytes: [u8; 2] = payload[start..end].try_into().expect("i16 payload");
    *cursor = end;
    i16::from_be_bytes(bytes)
}

fn read_i32(payload: &[u8], cursor: &mut usize) -> i32 {
    let start = *cursor;
    let end = start + 4;
    let bytes: [u8; 4] = payload[start..end].try_into().expect("i32 payload");
    *cursor = end;
    i32::from_be_bytes(bytes)
}
