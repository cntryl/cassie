//! Owner and observer wire sessions for deterministic cancellation recovery tests.
#![allow(dead_code)]

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::support_pgwire as wire;

pub type Frames = Vec<(u8, Vec<u8>)>;

const READ_LIMIT: Duration = Duration::from_secs(5);

pub struct CancelSession {
    server: wire::PgwireServer,
    owner_reader: OwnedReadHalf,
    owner_writer: OwnedWriteHalf,
    observer_reader: OwnedReadHalf,
    observer_writer: OwnedWriteHalf,
    process_id: i32,
    secret_key: i32,
}

#[must_use]
pub fn ready_status(frames: &Frames) -> Option<u8> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'Z')
        .and_then(|(_, body)| body.first().copied())
}

#[must_use]
pub fn command_tag(frames: &Frames) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, body)| {
            String::from_utf8_lossy(body)
                .trim_end_matches('\0')
                .to_owned()
        })
}

/// First data cell of the first row, or `None` for an empty or failed result.
#[must_use]
pub fn scalar(frames: &Frames) -> Option<String> {
    wire::data_rows(frames)
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next())
        .flatten()
}

impl CancelSession {
    /// # Panics
    ///
    /// Panics when the in-memory server or either connection cannot start.
    pub async fn start() -> Self {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        let owner = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("owner connection");
        let (mut owner_reader, mut owner_writer) = owner.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut owner_reader, &mut owner_writer).await;
        let observer = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("observer connection");
        let (mut observer_reader, mut observer_writer) = observer.into_split();
        wire::complete_startup(&mut observer_reader, &mut observer_writer).await;
        Self {
            server,
            owner_reader,
            owner_writer,
            observer_reader,
            observer_writer,
            process_id,
            secret_key,
        }
    }

    pub async fn owner(&mut self, sql: &str) -> Frames {
        wire::write_frames(&mut self.owner_writer, vec![wire::simple_query_frame(sql)]).await;
        wire::read_frames_until_ready_within(&mut self.owner_reader, READ_LIMIT).await
    }

    pub async fn observer(&mut self, sql: &str) -> Frames {
        wire::write_frames(
            &mut self.observer_writer,
            vec![wire::simple_query_frame(sql)],
        )
        .await;
        wire::read_frames_until_ready_within(&mut self.observer_reader, READ_LIMIT).await
    }

    /// Send raw frames on the owner connection and read through `ReadyForQuery`.
    pub async fn owner_frames(&mut self, frames: Vec<Vec<u8>>) -> Frames {
        wire::write_frames(&mut self.owner_writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.owner_reader, READ_LIMIT).await
    }

    /// Parse, Bind and Sync a named statement without executing it.
    pub async fn owner_prepare(&mut self, name: &str, sql: &str) -> Frames {
        wire::write_frames(
            &mut self.owner_writer,
            vec![
                wire::parse_frame(name, sql),
                wire::bind_frame(name, name, &[]),
                wire::sync_frame(),
            ],
        )
        .await;
        wire::read_frames_until_ready_within(&mut self.owner_reader, READ_LIMIT).await
    }

    /// Parse and Bind a named statement, then Flush, leaving the cycle open for Execute.
    ///
    /// # Panics
    ///
    /// Panics when the server answers with anything other than `ParseComplete` and `BindComplete`.
    pub async fn owner_parse_bind_open(&mut self, name: &str, sql: &str) {
        wire::write_frames(
            &mut self.owner_writer,
            vec![
                wire::parse_frame(name, sql),
                wire::bind_frame(name, name, &[]),
                wire::flush_frame(),
            ],
        )
        .await;
        for expected in *b"12" {
            let (tag, _) =
                tokio::time::timeout(READ_LIMIT, wire::read_wire_frame(&mut self.owner_reader))
                    .await
                    .expect("Parse/Bind response");
            assert_eq!(tag, expected, "Parse/Bind response order");
        }
    }

    /// Execute a bound portal and Sync.
    pub async fn owner_execute(&mut self, portal: &str) -> Frames {
        wire::write_frames(
            &mut self.owner_writer,
            vec![wire::execute_frame(portal), wire::sync_frame()],
        )
        .await;
        wire::read_frames_until_ready_within(&mut self.owner_reader, READ_LIMIT).await
    }

    /// Run `frames` on the owner, cancel once the statement enters its sleep, and
    /// return the owner's frames plus the observer's `probe` taken while paused.
    ///
    /// # Panics
    ///
    /// Panics when the statement never reaches the controlled sleep boundary.
    pub async fn cancel_at_sleep(&mut self, frames: Vec<Vec<u8>>, probe: &str) -> (Frames, Frames) {
        let (_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
        wire::write_frames(&mut self.owner_writer, frames).await;
        tokio::task::spawn_blocking(move || selected.recv_timeout(READ_LIMIT))
            .await
            .expect("sleep-entry observer task")
            .expect("statement must reach its controlled sleep boundary");
        let during = self.observer(probe).await;
        self.send_cancel().await;
        resume.send(()).expect("resume the paused statement");
        let canceled =
            wire::read_frames_until_ready_within(&mut self.owner_reader, READ_LIMIT).await;
        (canceled, during)
    }

    async fn send_cancel(&self) {
        let mut socket = tokio::net::TcpStream::connect(self.server.addr)
            .await
            .expect("CancelRequest socket");
        socket
            .write_all(&wire::cancel_request_frame(
                self.process_id,
                self.secret_key,
            ))
            .await
            .expect("send CancelRequest");
        socket.shutdown().await.expect("close CancelRequest socket");
        let mut response = [0; 1];
        let received = tokio::time::timeout(READ_LIMIT, socket.read(&mut response))
            .await
            .expect("CancelRequest handled")
            .expect("read CancelRequest close");
        assert_eq!(received, 0, "CancelRequest has no response body");
    }

    pub async fn finish(self) {
        let Self {
            server,
            owner_reader,
            owner_writer,
            observer_reader,
            observer_writer,
            ..
        } = self;
        drop((owner_reader, owner_writer, observer_reader, observer_writer));
        server.stop().await;
    }
}
