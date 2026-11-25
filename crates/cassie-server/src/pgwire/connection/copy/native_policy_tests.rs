//! Real wire COPY observes its owning Data submission on the COMMIT worker.
use super::super::blocking::arm;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn read_frame(socket: &mut tokio::net::TcpStream) -> (u8, Vec<u8>) {
    let tag = socket.read_u8().await.unwrap();
    let size = usize::try_from(socket.read_u32().await.unwrap()).unwrap();
    let mut body = vec![0; size - 4];
    socket.read_exact(&mut body).await.unwrap();
    (tag, body)
}

async fn send_frame(socket: &mut tokio::net::TcpStream, tag: u8, body: &[u8]) {
    socket.write_all(&[tag]).await.unwrap();
    socket
        .write_all(&u32::try_from(body.len() + 4).unwrap().to_be_bytes())
        .await
        .unwrap();
    socket.write_all(body).await.unwrap();
}

async fn ready(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    loop {
        let (tag, body) = read_frame(socket).await;
        assert_ne!(tag, b'E', "wire error {body:?}");
        if tag == b'Z' {
            return body;
        }
    }
}

async fn query(socket: &mut tokio::net::TcpStream, sql: &str) -> Vec<u8> {
    let mut body = sql.as_bytes().to_vec();
    body.push(0);
    send_frame(socket, b'Q', &body).await;
    ready(socket).await
}

async fn authenticate(socket: &mut tokio::net::TcpStream, password: &str) {
    let mut startup = 196_608_u32.to_be_bytes().to_vec();
    startup.extend_from_slice(b"user\0root\0database\0postgres\0\0");
    socket
        .write_all(&u32::try_from(startup.len() + 4).unwrap().to_be_bytes())
        .await
        .unwrap();
    socket.write_all(&startup).await.unwrap();
    assert_eq!(read_frame(socket).await.0, b'R');
    let mut password = password.as_bytes().to_vec();
    password.push(0);
    send_frame(socket, b'p', &password).await;
    assert_eq!(ready(socket).await, b"I");
}

fn assert_wire_copy_policy(sequence_default: bool, mode: &str, expected: &str) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        // Arrange
        let config = CassieRuntimeConfig { password: uuid::Uuid::new_v4().to_string(), ..CassieRuntimeConfig::default() };
        let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).unwrap());
        cassie.startup().unwrap();
        let observer = cassie.create_session("root", None);
        cassie.execute_sql(&observer, "CREATE TABLE copy_policy_checkpoint (position INT)", vec![]).unwrap();
        let rows_ddl = if sequence_default {
            cassie.execute_sql(&observer, "CREATE SEQUENCE copy_policy_sequence", vec![]).unwrap();
            "CREATE TABLE copy_policy_rows (id BIGINT DEFAULT nextval('copy_policy_sequence'), n INT)"
        } else { "CREATE TABLE copy_policy_rows (n INT)" };
        cassie.execute_sql(&observer, rows_ddl, vec![]).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let server = tokio::spawn(crate::pgwire::server::run_with_listener_and_shutdown(listener, Arc::clone(&cassie), config.clone(), Arc::clone(&shutdown)));
        let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), authenticate(&mut client, &config.password)).await.unwrap();
        assert_eq!(query(&mut client, "BEGIN").await, b"T");
        assert_eq!(query(&mut client, &format!("SET LOCAL cassie.projection_write_mode = '{mode}'")).await, b"T");
        send_frame(&mut client, b'Q', b"COPY copy_policy_rows (n) FROM STDIN\0").await;
        let (tag, body) = tokio::time::timeout(Duration::from_secs(10), read_frame(&mut client)).await.unwrap();
        assert_eq!(tag, b'G', "COPY admission {body:?}");
        send_frame(&mut client, b'd', b"41\n").await;
        send_frame(&mut client, b'c', &[]).await;
        assert_eq!(tokio::time::timeout(Duration::from_secs(10), ready(&mut client)).await.unwrap(), b"T");
        assert_eq!(query(&mut client, "INSERT INTO copy_policy_checkpoint VALUES (7)").await, b"T");
        for sql in ["SELECT n FROM copy_policy_rows", "SELECT position FROM copy_policy_checkpoint"] {
            assert!(cassie.execute_sql(&observer, sql, vec![]).unwrap().rows().is_empty(), "staged data must remain private");
        }
        let reactor = std::thread::current().id();
        let observation = arm(&cassie, "pgwire_simple_query");

        // Act
        let state = tokio::time::timeout(Duration::from_secs(10), query(&mut client, "COMMIT")).await.unwrap();
        let report = tokio::time::timeout(Duration::from_secs(10), observation.report).await.unwrap().unwrap();
        let rows = cassie.execute_sql(&observer, "SELECT n FROM copy_policy_rows", vec![]).unwrap();
        let checkpoint = cassie.execute_sql(&observer, "SELECT position FROM copy_policy_checkpoint", vec![]).unwrap();
        let assigned = sequence_default.then(|| cassie.execute_sql(&observer, "SELECT id FROM copy_policy_rows", vec![]).unwrap());
        send_frame(&mut client, b'X', &[]).await;
        shutdown.notify_one();
        server.await.unwrap().unwrap();

        // Assert
        assert_eq!(state, b"I");
        assert_ne!(report.worker, reactor, "observe actual blocking worker, not caller thread");
        assert_eq!(report.data.len(), 1, "one owning Data native attempt: {report:?}");
        assert_eq!(report.data[0].policy, expected, "Data first submission is independent of earlier schema epoch or maintenance: {report:?}");
        assert!(report.data[0].succeeded, "actual native Data result: {report:?}");
        assert_eq!(report.native.failed, 0);
        assert_eq!(report.native.outcome_unknown, 0);
        assert_eq!(report.native.best_effort_attempts, 0);
        assert!(report.native.first_policy.is_some());
        assert_eq!(report.native.attempted, report.native.primary_attempts + report.native.column_maintenance_attempts + report.native.hash_maintenance_attempts + report.native.time_series_maintenance_attempts);
        assert_eq!(rows.rows(), &[vec![Value::Int64(41)]]);
        assert_eq!(checkpoint.rows(), &[vec![Value::Int64(7)]]);
        if let Some(assigned) = assigned { assert_eq!(assigned.rows(), &[vec![Value::Int64(1)]]); }
    });
}

#[test]
fn should_use_strict_native_data_policy_for_default_wire_copy_transaction() {
    // Arrange / Act / Assert: helper stages real wire COPY + checkpoint, then observes actual COMMIT worker.
    assert_wire_copy_policy(false, "default", "local_sync");
}

#[test]
fn should_use_buffered_native_data_policy_for_replayable_wire_copy_transaction() {
    // Arrange / Act / Assert: helper proves private staging and the successful owning Data buffered attempt.
    assert_wire_copy_policy(false, "replayable", "local_buffered");
}

#[test]
fn should_promote_replayable_wire_copy_with_a_sequence_default_to_strict_data_policy() {
    // Arrange / Act / Assert: actual COPY default consumes one sequence value and promotes its later Data COMMIT.
    assert_wire_copy_policy(true, "replayable", "local_sync");
}
