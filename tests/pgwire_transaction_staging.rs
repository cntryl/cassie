use std::time::Duration;

use cassie::app::Cassie;

#[path = "support/sql.rs"]
mod sql;
#[path = "support/pgwire.rs"]
mod wire;

fn simple_query_frame(query: &str) -> Vec<u8> {
    let mut payload = query.as_bytes().to_vec();
    payload.push(0);
    let mut frame = vec![b'Q'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("simple query payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

#[test]
fn should_commit_multi_collection_staging_with_transaction_ready_status() {
    // Arrange
    sql::with_fallback();
    let path = sql::data_dir("pgwire_transaction_staging");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE pgwire_stage_a (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create first collection");
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE pgwire_stage_b (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .expect("create second collection");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(&mut writer, vec![simple_query_frame("BEGIN")]).await;
        let begin_frames = wire::read_frames_until_ready(&mut reader).await;

        wire::write_frames(
            &mut writer,
            vec![simple_query_frame(
                "INSERT INTO pgwire_stage_a (id, title) VALUES (1, 'alpha')",
            )],
        )
        .await;
        let first_frames = wire::read_frames_until_ready(&mut reader).await;

        // Act
        wire::write_frames(
            &mut writer,
            vec![simple_query_frame(
                "INSERT INTO pgwire_stage_b (id, title) VALUES (1, 'beta')",
            )],
        )
        .await;
        let second_frames = wire::read_frames_until_ready(&mut reader).await;

        wire::write_frames(&mut writer, vec![simple_query_frame("COMMIT")]).await;
        let commit_frames = wire::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(begin_frames.last().expect("begin ready").1, vec![b'T']);
        assert_eq!(first_frames.last().expect("first ready").1, vec![b'T']);
        assert!(!second_frames.iter().any(|(tag, _)| *tag == b'E'));
        assert_eq!(second_frames.last().expect("second ready").1, vec![b'T']);
        assert_eq!(commit_frames.last().expect("commit ready").1, vec![b'I']);
        assert_eq!(
            setup.transaction_status(),
            "idle",
            "setup session remains independent"
        );

        server.stop().await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        let _ = std::fs::remove_dir_all(path);
    });
}
