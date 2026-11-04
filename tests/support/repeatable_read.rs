//! Two independent wire connections around a first-snapshot boundary.

use std::time::Duration;

use crate::support_pgwire as wire;

pub type Frames = Vec<(u8, Vec<u8>)>;

pub fn run_boundary(
    before_begin: Vec<Vec<u8>>,
    boundary: Vec<Vec<u8>>,
    readback: Vec<Vec<u8>>,
) -> (Frames, Frames) {
    let (mut retained, fresh) = run_script(vec![before_begin], boundary, vec![readback]);
    (retained.pop().expect("one readback"), fresh)
}

pub fn run_script(
    before_begin: Vec<Vec<Vec<u8>>>,
    boundary: Vec<Vec<u8>>,
    readbacks: Vec<Vec<Vec<u8>>>,
) -> (Vec<Frames>, Frames) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rr_boundary_rows (id INT PRIMARY KEY)",
            "INSERT INTO rr_boundary_rows VALUES (1)",
        ] {
            cassie.execute_sql(&session, sql, Vec::new()).expect("seed");
        }
        let server = wire::spawn_server(cassie).await;
        let mut reader = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("reader");
        let mut writer = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("writer");
        let (mut reader_read, mut reader_write) = tokio::io::split(&mut reader);
        let (mut writer_read, mut writer_write) = tokio::io::split(&mut writer);
        wire::complete_startup(&mut reader_read, &mut reader_write).await;
        wire::complete_startup(&mut writer_read, &mut writer_write).await;
        for frames in before_begin.into_iter().filter(|frames| !frames.is_empty()) {
            exchange(&mut reader_read, &mut reader_write, frames).await;
        }
        exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame(
                "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            )],
        )
        .await;
        exchange(&mut reader_read, &mut reader_write, boundary).await;
        exchange(
            &mut writer_read,
            &mut writer_write,
            vec![wire::simple_query_frame(
                "INSERT INTO rr_boundary_rows VALUES (3)",
            )],
        )
        .await;
        let mut retained = Vec::new();
        for frames in readbacks {
            retained.push(exchange(&mut reader_read, &mut reader_write, frames).await);
        }
        exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame("COMMIT")],
        )
        .await;
        let fresh = exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame(
                "SELECT count(*) FROM rr_boundary_rows",
            )],
        )
        .await;
        drop((
            reader_read,
            reader_write,
            writer_read,
            writer_write,
            reader,
            writer,
        ));
        server.stop().await;
        (retained, fresh)
    })
}

async fn exchange(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: Vec<Vec<u8>>,
) -> Frames {
    wire::write_frames(writer, frames).await;
    let frames = wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
    assert_eq!(wire::error_code(&frames), None, "{frames:?}");
    frames
}

pub fn assert_counts((retained, fresh): &(Frames, Frames), expected: &str) {
    assert_eq!(
        wire::data_rows(retained),
        vec![vec![Some(expected.to_owned())]]
    );
    assert_eq!(wire::data_rows(fresh), vec![vec![Some("2".to_owned())]]);
}

pub fn disconnect_retained_view() -> (u64, u64) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let observer = cassie.clone();
        let server = wire::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("reader");
        let (mut reader, mut writer) = tokio::io::split(&mut socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        exchange(
            &mut reader,
            &mut writer,
            vec![wire::simple_query_frame(
                "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SELECT 1",
            )],
        )
        .await;
        let retained = observer.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("retained charge");

        wire::write_frames(&mut writer, vec![vec![b'X', 0, 0, 0, 4]]).await;
        let mut byte = [0];
        let eof = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncReadExt::read(&mut reader, &mut byte),
        )
        .await
        .expect("Terminate closes connection after state cleanup")
        .expect("EOF");
        assert_eq!(eof, 0);
        let released = observer.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("released charge");
        drop((reader, writer, socket));
        server.stop().await;
        (retained, released)
    })
}
