use cassie::app::Cassie;
use tokio::io::AsyncWriteExt;

#[path = "support/pgwire.rs"]
mod support;

fn configured_cassie(label: &str) -> (Cassie, String) {
    support::use_local_storage();
    let path = support::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE pgwire_copy_recovery_rows (id INT, title TEXT)",
            vec![],
        )
        .expect("create copy table");
    (cassie, path)
}

#[test]
fn should_resume_simple_query_processing_after_copy_error() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-simple-recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        write_half
            .write_all(&support::simple_query_frame(
                "COPY pgwire_copy_recovery_rows (id, title) FROM STDIN WITH (FORMAT csv)",
            ))
            .await
            .expect("request copy");
        write_half.flush().await.expect("flush copy request");
        assert_eq!(support::read_wire_frame(&mut reader).await.0, b'G');

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::copy_data_frame(b"not-an-integer,broken\n"),
                support::copy_done_frame(),
            ],
        )
        .await;
        let failed = support::read_frames_until_ready(&mut reader).await;
        write_half
            .write_all(&support::simple_query_frame("SELECT 1"))
            .await
            .expect("recovery query");
        write_half.flush().await.expect("flush recovery query");
        let recovered = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert!(failed.iter().any(|frame| frame.0 == b'E'));
        assert!(recovered.iter().any(|frame| frame.0 == b'D'));
        assert!(recovered.iter().any(|frame| frame.0 == b'C'));
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_invalidate_extended_portals_after_sync_given_a_copy_failure() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-extended-recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("kept_statement", "SELECT 1"),
                support::bind_frame("stale_portal", "kept_statement", &[]),
                support::sync_frame(),
            ],
        )
        .await;
        let prepared = support::read_frames_until_ready(&mut reader).await;
        assert!(prepared.iter().any(|frame| frame.0 == b'2'));

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame(
                    "copy_statement",
                    "COPY pgwire_copy_recovery_rows FROM STDIN WITH (FORMAT csv)",
                ),
                support::sync_frame(),
            ],
        )
        .await;
        let failed = support::read_frames_until_ready(&mut reader).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_frame("stale_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        let after_sync = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert!(failed.iter().any(|frame| frame.0 == b'E'));
        assert!(after_sync.iter().any(|frame| frame.0 == b'E'));
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}
