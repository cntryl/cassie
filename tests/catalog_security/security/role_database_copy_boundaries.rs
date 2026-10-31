use super::support_pgwire as support;

use tokio::io::AsyncWriteExt;

fn database_copy_error(sql: &str) -> Vec<(char, String)> {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("admin");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE image_reader LOGIN PASSWORD 'reader-secret'",
            vec![],
        )
        .expect("create reader");
    cassie
        .execute_sql(&admin, "CREATE DATABASE denied_copy", vec![])
        .expect("create denied database");
    cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE postgres TO image_reader",
            vec![],
        )
        .expect("grant scoped database access");
    cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE denied_copy TO image_reader",
            vec![],
        )
        .expect("grant target database access");
    assert!(cassie
        .catalog
        .get_role("image_reader")
        .is_some_and(|role| role.can_access_database("denied_copy")));
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
    fields
}

#[test]
fn should_reject_non_admin_database_backup_even_with_connect_grant() {
    // Arrange
    let sql = "BACKUP DATABASE denied_copy TO STDOUT";

    // Act
    let fields = database_copy_error(sql);

    // Assert
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'C' && value == "42501"));
}

#[test]
fn should_reject_non_admin_database_restore_even_with_connect_grant() {
    // Arrange
    let sql = "RESTORE DATABASE denied_copy FROM STDIN";

    // Act
    let fields = database_copy_error(sql);

    // Assert
    assert!(fields
        .iter()
        .any(|(kind, value)| *kind == 'C' && value == "42501"));
}
