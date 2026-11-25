use super::support_pgwire as wire;

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

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

#[test]
fn should_report_transaction_semantics_sqlstate_through_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("startup");
        let setup = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &setup,
                "CREATE TABLE pgwire_transaction_semantics_source (title TEXT)",
                vec![],
            )
            .expect("create source table");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        // Act
        wire::write_frames(
            &mut writer,
            vec![simple_query_frame("BEGIN ISOLATION LEVEL SERIALIZABLE")],
        )
        .await;
        let frames = wire::read_frames_until_ready(&mut reader).await;

        // Assert
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("transaction isolation error");
        let fields = wire::parse_error_fields(&error.1);
        assert_eq!(error_field(&fields, 'C'), Some("0A000"));
        assert_eq!(frames.last().expect("idle ready").1, vec![b'I']);

        wire::write_frames(&mut writer, vec![simple_query_frame("BEGIN")]).await;
        let begin_frames = wire::read_frames_until_ready(&mut reader).await;
        assert_eq!(begin_frames.last().expect("active ready").1, vec![b'T']);

        wire::write_frames(
            &mut writer,
            vec![simple_query_frame(
                "CREATE TABLE pgwire_transaction_semantics_staged (value TEXT)",
            )],
        )
        .await;
        let ddl_frames = wire::read_frames_until_ready(&mut reader).await;
        assert!(ddl_frames.iter().all(|(tag, _)| *tag != b'E'));
        assert_eq!(ddl_frames.last().expect("active DDL ready").1, vec![b'T']);
        wire::write_frames(&mut writer, vec![simple_query_frame(
            "INSERT INTO pgwire_transaction_semantics_staged VALUES ('staged'); SELECT value FROM pgwire_transaction_semantics_staged"
        )]).await;
        let staged = wire::read_frames_until_ready(&mut reader).await;
        assert!(staged.iter().all(|(tag, _)| *tag != b'E'));
        assert_eq!(wire::data_rows(&staged), vec![vec![Some("staged".to_owned())]]);
        assert_eq!(staged.last().expect("active staged ready").1, vec![b'T']);

        wire::write_frames(&mut writer, vec![simple_query_frame("ROLLBACK")]).await;
        let rollback_frames = wire::read_frames_until_ready(&mut reader).await;
        assert_eq!(
            rollback_frames.last().expect("rollback ready").1,
            vec![b'I']
        );
        wire::write_frames(&mut writer, vec![simple_query_frame(
            "SELECT value FROM pgwire_transaction_semantics_staged"
        )]).await;
        let absent = wire::read_frames_until_ready(&mut reader).await;
        let missing = absent.iter().find(|(tag, _)| *tag == b'E').expect("rolled-back table must be absent");
        assert_eq!(error_field(&wire::parse_error_fields(&missing.1), 'C'), Some("42P01"));
        assert_eq!(absent.last().expect("idle error ready").1, vec![b'I']);
        wire::write_frames(&mut writer, vec![simple_query_frame("SELECT 1")]).await;
        let recovered = wire::read_frames_until_ready(&mut reader).await;
        assert_eq!(wire::data_rows(&recovered), vec![vec![Some("1".to_owned())]]);
        assert_eq!(recovered.last().expect("recovered idle ready").1, vec![b'I']);
        server.stop().await;
    });
}
