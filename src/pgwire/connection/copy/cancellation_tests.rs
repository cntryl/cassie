use std::cell::RefCell;
use std::future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::OnceLock;
use std::task::{Context, Poll};
use std::time::Duration;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use super::{select_copy_read, CopyReadOutcome, FrontendMessage, HandshakeError};

struct ReadProgressObserver<R> {
    inner: R,
    progress: tokio::sync::mpsc::UnboundedSender<usize>,
    bytes_read: usize,
}

impl<R> ReadProgressObserver<R> {
    fn new(inner: R, progress: tokio::sync::mpsc::UnboundedSender<usize>) -> Self {
        Self {
            inner,
            progress,
            bytes_read: 0,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ReadProgressObserver<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let filled_before = buffer.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(context, buffer);
        if let Poll::Ready(Ok(())) = &result {
            let newly_read = buffer.filled().len() - filled_before;
            if newly_read > 0 {
                this.bytes_read += newly_read;
                let _ = this.progress.send(this.bytes_read);
            }
        }
        result
    }
}

thread_local! {
    static COPY_PAYLOAD_BUFFERED: RefCell<Option<tokio::sync::oneshot::Sender<()>>> = const {
        RefCell::new(None)
    };
    static FRONTEND_COPY_TAG_READ: RefCell<Option<tokio::sync::oneshot::Sender<()>>> = const {
        RefCell::new(None)
    };
    static FRONTEND_COPY_PAYLOAD_READ: RefCell<Option<tokio::sync::oneshot::Sender<usize>>> = const {
        RefCell::new(None)
    };
}

struct CopyPayloadBufferedObserverGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for CopyPayloadBufferedObserverGuard {
    fn drop(&mut self) {
        COPY_PAYLOAD_BUFFERED.with(|observer| {
            observer.borrow_mut().take();
        });
    }
}

struct FrontendCopyTagReadObserverGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for FrontendCopyTagReadObserverGuard {
    fn drop(&mut self) {
        FRONTEND_COPY_TAG_READ.with(|observer| {
            observer.borrow_mut().take();
        });
    }
}

struct FrontendCopyPayloadReadObserverGuard {
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for FrontendCopyPayloadReadObserverGuard {
    fn drop(&mut self) {
        FRONTEND_COPY_PAYLOAD_READ.with(|observer| {
            observer.borrow_mut().take();
        });
    }
}

fn observe_next_copy_payload_buffered() -> (
    tokio::sync::oneshot::Receiver<()>,
    CopyPayloadBufferedObserverGuard,
) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    COPY_PAYLOAD_BUFFERED.with(|observer| {
        assert!(
            observer.borrow_mut().replace(sender).is_none(),
            "COPY payload observers must not be nested"
        );
    });
    (
        receiver,
        CopyPayloadBufferedObserverGuard {
            _thread_bound: PhantomData,
        },
    )
}

fn observe_next_frontend_copy_tag_read() -> (
    tokio::sync::oneshot::Receiver<()>,
    FrontendCopyTagReadObserverGuard,
) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    FRONTEND_COPY_TAG_READ.with(|observer| {
        assert!(
            observer.borrow_mut().replace(sender).is_none(),
            "frontend COPY tag observers must not be nested"
        );
    });
    (
        receiver,
        FrontendCopyTagReadObserverGuard {
            _thread_bound: PhantomData,
        },
    )
}

fn observe_next_frontend_copy_payload_read() -> (
    tokio::sync::oneshot::Receiver<usize>,
    FrontendCopyPayloadReadObserverGuard,
) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    FRONTEND_COPY_PAYLOAD_READ.with(|observer| {
        assert!(
            observer.borrow_mut().replace(sender).is_none(),
            "frontend COPY payload observers must not be nested"
        );
    });
    (
        receiver,
        FrontendCopyPayloadReadObserverGuard {
            _thread_bound: PhantomData,
        },
    )
}

pub(super) fn signal_copy_payload_buffered() {
    COPY_PAYLOAD_BUFFERED.with(|observer| {
        if let Some(sender) = observer.borrow_mut().take() {
            let _ = sender.send(());
        }
    });
}

pub(in crate::pgwire::connection) fn signal_frontend_copy_tag_read(tag: u8) {
    if tag != b'd' {
        return;
    }
    FRONTEND_COPY_TAG_READ.with(|observer| {
        if let Some(sender) = observer.borrow_mut().take() {
            let _ = sender.send(());
        }
    });
}

pub(in crate::pgwire::connection) fn signal_frontend_copy_payload_read(tag: u8, bytes: usize) {
    if tag != b'd' {
        return;
    }
    FRONTEND_COPY_PAYLOAD_READ.with(|observer| {
        if let Some(sender) = observer.borrow_mut().take() {
            let _ = sender.send(bytes);
        }
    });
}

fn frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("frontend frame length")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn startup_frame() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&196_608_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0root\0database\0postgres\0\0");
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup frame length")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_backend_frame(reader: &mut (impl AsyncRead + Unpin)) -> (u8, Vec<u8>) {
    let mut tag = [0_u8; 1];
    reader
        .read_exact(&mut tag)
        .await
        .expect("backend message tag");
    let mut length = [0_u8; 4];
    reader
        .read_exact(&mut length)
        .await
        .expect("backend message length");
    let body_len =
        usize::try_from(i32::from_be_bytes(length) - 4).expect("valid backend message length");
    let mut body = vec![0_u8; body_len];
    reader
        .read_exact(&mut body)
        .await
        .expect("backend message body");
    (tag[0], body)
}

async fn read_backend_frames_until_ready(
    reader: &mut (impl AsyncRead + Unpin),
) -> Vec<(u8, Vec<u8>)> {
    let mut frames = Vec::new();
    loop {
        let frame = read_backend_frame(reader).await;
        let ready = frame.0 == b'Z';
        frames.push(frame);
        if ready {
            return frames;
        }
    }
}

async fn write_frontend_frames(writer: &mut (impl AsyncWrite + Unpin), frames: Vec<Vec<u8>>) {
    let mut bytes = Vec::new();
    for frame in frames {
        bytes.extend_from_slice(&frame);
    }
    writer.write_all(&bytes).await.expect("frontend frames");
}

async fn complete_startup(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
) -> (i32, i32) {
    writer
        .write_all(&startup_frame())
        .await
        .expect("send startup frame");
    let (auth_tag, auth_body) = read_backend_frame(reader).await;
    assert_eq!(auth_tag, b'R');
    assert_eq!(auth_body, 3_i32.to_be_bytes());
    let mut password = test_root_password().as_bytes().to_vec();
    password.push(0);
    write_frontend_frames(writer, vec![frontend_frame(b'p', &password)]).await;

    let mut backend_key = None;
    loop {
        let frame = read_backend_frame(reader).await;
        if frame.0 == b'K' {
            backend_key = Some((
                i32::from_be_bytes(frame.1[..4].try_into().expect("backend process id")),
                i32::from_be_bytes(frame.1[4..].try_into().expect("backend secret key")),
            ));
        }
        if frame.0 == b'Z' {
            return backend_key.expect("backend key data before ReadyForQuery");
        }
    }
}

fn test_root_password() -> &'static str {
    static PASSWORD: OnceLock<String> = OnceLock::new();
    PASSWORD
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .as_str()
}

async fn cancel_backend(server_addr: std::net::SocketAddr, process_id: i32, secret_key: i32) {
    let mut cancel_socket = tokio::net::TcpStream::connect(server_addr)
        .await
        .expect("connect cancellation request");
    let mut frame = Vec::with_capacity(16);
    frame.extend_from_slice(&16_i32.to_be_bytes());
    frame.extend_from_slice(&80_877_102_i32.to_be_bytes());
    frame.extend_from_slice(&process_id.to_be_bytes());
    frame.extend_from_slice(&secret_key.to_be_bytes());
    cancel_socket
        .write_all(&frame)
        .await
        .expect("send CancelRequest");
    cancel_socket
        .shutdown()
        .await
        .expect("close cancellation request");
    let mut response = [0_u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), cancel_socket.read(&mut response))
        .await
        .expect("server should process CancelRequest");
    assert_eq!(read.expect("read cancellation close"), 0);
}

fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
    let mut cursor = 0;
    while cursor < payload.len() && payload[cursor] != 0 {
        let kind = payload[cursor];
        cursor += 1;
        let end = payload[cursor..].iter().position(|byte| *byte == 0)? + cursor;
        if kind == b'C' {
            return Some(String::from_utf8_lossy(&payload[cursor..end]).into_owned());
        }
        cursor = end + 1;
    }
    None
}

#[test]
fn should_prioritize_cancellation_given_a_ready_copy_message() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let message = future::ready(Ok::<_, HandshakeError>(FrontendMessage::CopyDone));

    // Act
    let outcome = runtime.block_on(select_copy_read(&cancellation, message));

    // Assert
    assert!(matches!(outcome, CopyReadOutcome::Cancelled));
}

struct PgwireCopyCancelHarness {
    cassie: std::sync::Arc<Cassie>,
    server: tokio::task::JoinHandle<Result<(), crate::app::CassieError>>,
    server_addr: std::net::SocketAddr,
    process_id: i32,
    secret_key: i32,
    reader: tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: tokio::net::tcp::OwnedWriteHalf,
    table: &'static str,
}

impl PgwireCopyCancelHarness {
    async fn start(table: &'static str, explicit_transaction: bool) -> Self {
        let config = CassieRuntimeConfig {
            password: test_root_password().to_owned(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = std::sync::Arc::new(
            Cassie::new_in_memory_with_config(config.clone()).expect("create in-memory Cassie"),
        );
        cassie.startup().expect("start Cassie");
        let setup = cassie.create_session("root", None);
        cassie
            .execute_sql(&setup, &format!("CREATE TABLE {table} (id INT)"), vec![])
            .expect("create COPY target");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let server_addr = listener.local_addr().expect("pgwire address");
        let server = tokio::spawn(crate::pgwire::server::run_with_listener(
            listener,
            std::sync::Arc::clone(&cassie),
            config,
        ));
        let socket = tokio::net::TcpStream::connect(server_addr)
            .await
            .expect("connect pgwire client");
        let (read_half, mut writer) = socket.into_split();
        let mut reader = tokio::io::BufReader::new(read_half);
        let (process_id, secret_key) = tokio::time::timeout(
            Duration::from_secs(5),
            complete_startup(&mut reader, &mut writer),
        )
        .await
        .expect("complete pgwire startup");
        let mut harness = Self {
            cassie,
            server,
            server_addr,
            process_id,
            secret_key,
            reader,
            writer,
            table,
        };
        if explicit_transaction {
            harness.begin_transaction().await;
        }
        harness.begin_copy().await;
        harness
    }

    async fn begin_transaction(&mut self) {
        write_frontend_frames(&mut self.writer, vec![frontend_frame(b'Q', b"BEGIN\0")]).await;
        let begun = self.read_until_ready("explicit transaction start").await;
        assert_eq!(
            begun.last().map(|(tag, body)| (*tag, body.as_slice())),
            Some((b'Z', b"T".as_slice()))
        );
    }

    async fn begin_copy(&mut self) {
        let query = format!("COPY {} (id) FROM STDIN\0", self.table);
        write_frontend_frames(
            &mut self.writer,
            vec![frontend_frame(b'Q', query.as_bytes())],
        )
        .await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), read_backend_frame(&mut self.reader))
                .await
                .expect("receive CopyInResponse")
                .0,
            b'G'
        );
    }

    async fn read_until_ready(&mut self, reason: &'static str) -> Vec<(u8, Vec<u8>)> {
        tokio::time::timeout(
            Duration::from_secs(5),
            read_backend_frames_until_ready(&mut self.reader),
        )
        .await
        .expect(reason)
    }

    async fn cancel(&self) {
        cancel_backend(self.server_addr, self.process_id, self.secret_key).await;
    }

    fn target_is_empty(&self) -> bool {
        let observer = self.cassie.create_session("root", None);
        self.cassie
            .execute_sql(&observer, &format!("SELECT id FROM {}", self.table), vec![])
            .expect("read target after cancelled COPY")
            .rows()
            .is_empty()
    }

    async fn recover_after_late_copy(&mut self, explicit_transaction: bool) -> Vec<(u8, Vec<u8>)> {
        let query = if explicit_transaction {
            b"ROLLBACK\0".as_slice()
        } else {
            b"SELECT 1\0".as_slice()
        };
        write_frontend_frames(
            &mut self.writer,
            vec![
                frontend_frame(b'd', b"8\n"),
                frontend_frame(b'c', &[]),
                frontend_frame(b'Q', query),
            ],
        )
        .await;
        let after_late_copy = self.read_until_ready("query after late COPY frames").await;
        if explicit_transaction {
            assert_eq!(
                after_late_copy
                    .last()
                    .map(|(tag, body)| (*tag, body.as_slice())),
                Some((b'Z', b"I".as_slice()))
            );
            write_frontend_frames(&mut self.writer, vec![frontend_frame(b'Q', b"SELECT 1\0")])
                .await;
            self.read_until_ready("query after rollback").await
        } else {
            after_late_copy
        }
    }

    async fn cancel_during_partial_frame(&mut self, after_body: bool) {
        let remaining_frame = if after_body {
            b"\n".to_vec()
        } else {
            let mut frame = 6_i32.to_be_bytes().to_vec();
            frame.extend_from_slice(b"7\n");
            frame
        };
        if after_body {
            let (payload_read, _payload_guard) = observe_next_frontend_copy_payload_read();
            let mut partial_frame = vec![b'd'];
            partial_frame.extend_from_slice(&6_i32.to_be_bytes());
            partial_frame.push(b'7');
            self.writer
                .write_all(&partial_frame)
                .await
                .expect("send tag, frame length, and part of the COPY body");
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(5), payload_read)
                    .await
                    .expect("server should consume part of the COPY body")
                    .expect("COPY payload read observation"),
                1
            );
        } else {
            let (tag_read, _tag_guard) = observe_next_frontend_copy_tag_read();
            self.writer
                .write_all(b"d")
                .await
                .expect("send CopyData tag");
            tokio::time::timeout(Duration::from_secs(5), tag_read)
                .await
                .expect("server should consume the COPY frame tag")
                .expect("COPY frame tag observation");
        }
        self.cancel().await;
        self.writer
            .write_all(&remaining_frame)
            .await
            .expect("finish the partially sent CopyData frame");
    }

    async fn shutdown(self) {
        let Self {
            reader,
            writer,
            server,
            ..
        } = self;
        drop((reader, writer));
        server.abort();
        let _ = server.await;
    }
}

fn assert_copy_cancelled(frames: &[(u8, Vec<u8>)], explicit_transaction: bool) {
    assert_eq!(error_code(frames).as_deref(), Some("57014"));
    assert_eq!(
        frames.last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((
            b'Z',
            if explicit_transaction {
                b"E".as_slice()
            } else {
                b"I".as_slice()
            }
        ))
    );
}

fn assert_copy_recovered(frames: &[(u8, Vec<u8>)], target_is_empty: bool, row_message: &str) {
    assert!(target_is_empty, "{row_message}");
    assert!(frames.iter().any(|(tag, _)| *tag == b'D'));
    assert_eq!(
        frames.last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((b'Z', b"I".as_slice()))
    );
}

async fn run_buffered_copy_cancel(explicit_transaction: bool) -> u8 {
    let mut harness =
        PgwireCopyCancelHarness::start("copy_buffer_cancel_probe", explicit_transaction).await;
    let (buffered, _buffered_guard) = observe_next_copy_payload_buffered();

    // Act
    write_frontend_frames(&mut harness.writer, vec![frontend_frame(b'd', b"7\n")]).await;
    tokio::time::timeout(Duration::from_secs(5), buffered)
        .await
        .expect("server should buffer CopyData before cancellation")
        .expect("COPY payload observation");
    harness.cancel().await;
    let cancelled = harness
        .read_until_ready("cancelled buffered COPY should return to query mode")
        .await;
    let target_is_empty = harness.target_is_empty();
    let recovered = harness.recover_after_late_copy(explicit_transaction).await;

    // Assert
    assert_copy_cancelled(&cancelled, explicit_transaction);
    assert_copy_recovered(
        &recovered,
        target_is_empty,
        "cancelled COPY must discard buffered rows",
    );
    harness.shutdown().await;
    if explicit_transaction {
        b'E'
    } else {
        b'I'
    }
}

#[test]
fn should_cancel_buffered_copy_inside_implicit_transaction() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    assert_eq!(runtime.block_on(run_buffered_copy_cancel(false)), b'I');

    // Assert
}

#[test]
fn should_cancel_buffered_copy_inside_explicit_transaction() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    assert_eq!(runtime.block_on(run_buffered_copy_cancel(true)), b'E');

    // Assert
}

async fn run_partial_copy_frame_cancel(
    cancel_after_partial_body: bool,
    explicit_transaction: bool,
) {
    let mut harness =
        PgwireCopyCancelHarness::start("copy_partial_cancel_probe", explicit_transaction).await;
    harness
        .cancel_during_partial_frame(cancel_after_partial_body)
        .await;
    let cancelled = harness
        .read_until_ready("cancelled COPY should return to query mode")
        .await;
    let target_is_empty = harness.target_is_empty();
    let recovered = harness.recover_after_late_copy(explicit_transaction).await;

    // Assert
    assert_copy_cancelled(&cancelled, explicit_transaction);
    assert_copy_recovered(
        &recovered,
        target_is_empty,
        "cancelled COPY must discard partial rows",
    );
    harness.shutdown().await;
}

#[test]
fn should_preserve_frontend_alignment_when_cancelled_after_copy_tag() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    runtime.block_on(run_partial_copy_frame_cancel(false, false));

    // Assert
}

#[test]
fn should_preserve_frontend_alignment_when_cancelled_after_partial_copy_body() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    runtime.block_on(run_partial_copy_frame_cancel(true, false));

    // Assert
}

#[test]
fn should_fail_explicit_transaction_after_partial_copy_body_cancellation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    runtime.block_on(run_partial_copy_frame_cancel(true, true));

    // Assert
}

async fn cancel_copy_read_after_partial_frame_prefix(
    prefix: &[u8],
    remaining_frame: &[u8],
) -> CopyReadOutcome<Result<FrontendMessage, HandshakeError>> {
    let (mut client, server) = tokio::io::duplex(64);
    let (progress_sender, mut progress) = tokio::sync::mpsc::unbounded_channel();
    let cancellation = QueryCancellationHandle::new();
    let reader_cancellation = cancellation.clone();
    let mut reader = tokio::io::BufReader::new(ReadProgressObserver::new(server, progress_sender));
    let read = tokio::spawn(async move {
        super::read_copy_frontend_message(&reader_cancellation, &mut reader).await
    });
    client
        .write_all(prefix)
        .await
        .expect("send partial frontend frame");
    loop {
        let bytes_read = tokio::time::timeout(Duration::from_secs(5), progress.recv())
            .await
            .expect("server should read the partial frame")
            .expect("reader progress observer remains open");
        if bytes_read >= prefix.len() {
            break;
        }
    }
    cancellation.cancel();
    client
        .write_all(remaining_frame)
        .await
        .expect("complete frontend frame after cancellation");
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .expect("reader should finish the frame after cancellation")
        .expect("COPY reader task")
}

#[test]
fn should_finish_partially_read_copy_body_before_applying_cancellation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let mut prefix = vec![b'd'];
    prefix.extend_from_slice(&6_i32.to_be_bytes());
    prefix.push(b'7');

    // Act
    let outcome = runtime.block_on(cancel_copy_read_after_partial_frame_prefix(&prefix, b"\n"));

    // Assert
    assert!(matches!(outcome, CopyReadOutcome::Cancelled));
}

#[test]
fn should_finish_partially_read_copy_length_before_applying_cancellation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let length = 6_i32.to_be_bytes();
    let mut prefix = vec![b'd'];
    prefix.extend_from_slice(&length[..2]);
    let mut remaining_frame = length[2..].to_vec();
    remaining_frame.extend_from_slice(b"7\n");

    // Act
    let outcome = runtime.block_on(cancel_copy_read_after_partial_frame_prefix(
        &prefix,
        &remaining_frame,
    ));

    // Assert
    assert!(matches!(outcome, CopyReadOutcome::Cancelled));
}
