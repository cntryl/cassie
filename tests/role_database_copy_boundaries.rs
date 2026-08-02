use cassie::app::Cassie;
use tokio::io::AsyncWriteExt;

#[path = "support/pgwire.rs"]
mod support;

fn database_copy_error(test_name: &str, sql: &str) -> Vec<(char, String)> {
    support::use_local_storage();
    let path = support::data_dir(test_name);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE image_reader LOGIN PASSWORD 'reader-secret'",
            vec![],
        )
        .expect("create reader");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let fields = runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup_as(
            &mut reader,
            &mut write_half,
            "image_reader",
            "postgres",
            "reader-secret",
        )
        .await;
        write_half
            .write_all(&support::simple_query_frame(sql))
            .await
            .expect("write database image query");
        write_half
            .flush()
            .await
            .expect("flush database image query");
        let frames = support::read_frames_until_ready(&mut reader).await;
        let error = frames
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("insufficient privilege error");
        let fields = support::parse_error_fields(&error.1);
        server.stop().await;
        fields
    });
    let _ = std::fs::remove_dir_all(path);
    fields
}

#[test]
fn should_reject_read_only_role_database_backup() {
    // Arrange
    let fields = database_copy_error("role-database-backup", "BACKUP DATABASE postgres TO STDOUT");

    // Act / Assert
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'C' && value == "42501"));
}

#[test]
fn should_reject_read_only_role_database_restore() {
    // Arrange
    let fields = database_copy_error(
        "role-database-restore",
        "RESTORE DATABASE restored FROM STDIN",
    );

    // Act / Assert
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'C' && value == "42501"));
}
