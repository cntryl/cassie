use crate::support_pgwire as wire;
use cassie::app::Cassie;
use tokio::io::BufReader;

#[test]
fn should_describe_alias_prefixes_and_resume_parameterized_portals() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("alias-wire");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE public.records (id INT, score BIGINT)",
            "INSERT INTO public.records VALUES (1, 10), (2, 20), (3, 30), (4, 40)",
        ] { cassie.execute_sql(&session, sql, vec![]).expect("setup"); }
        let server = wire::spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr).await.expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = BufReader::new(read_half);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::transaction_control(&mut reader, &mut writer, "BEGIN").await;

        // Act
        wire::write_frames(&mut writer, vec![
            wire::parse_frame("aliased", "SELECT \"R\".key, \"R\".score FROM public.records AS \"R\"(key) WHERE \"R\".key >= $1 ORDER BY \"R\".key LIMIT $2 OFFSET $3"),
            wire::describe_statement_frame("aliased"),
            wire::bind_frame("aliased_portal", "aliased", &["1", "3", "1"]),
            wire::describe_portal_frame("aliased_portal"),
            wire::execute_limited_frame("aliased_portal", 2),
            wire::sync_frame(),
        ]).await;
        let first = wire::read_frames_until_ready(&mut reader).await;
        wire::write_frames(&mut writer, vec![wire::execute_limited_frame("aliased_portal", 2), wire::sync_frame()]).await;
        let second = wire::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(wire::error_code(&first), None);
        let parameters = first.iter().find(|frame| frame.0 == b't').expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23, 20, 20]);
        let descriptions = first.iter().filter(|frame| frame.0 == b'T')
            .map(|frame| wire::parse_row_description(&frame.1)).collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        assert_eq!(descriptions[0], descriptions[1]);
        assert_eq!(descriptions[0].iter().map(|column| (column.name.as_str(), column.type_oid)).collect::<Vec<_>>(),
            vec![("key", 23), ("score", 20)]);
        assert_eq!(wire::data_rows(&first), vec![vec![Some("2".into()), Some("20".into())], vec![Some("3".into()), Some("30".into())]]);
        assert!(first.iter().any(|frame| frame.0 == b's'));
        assert_eq!(wire::error_code(&second), None);
        assert_eq!(wire::data_rows(&second), vec![vec![Some("4".into()), Some("40".into())]]);
        assert!(second.iter().any(|frame| frame.0 == b'C'));
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("COMMIT")]).await;
        let committed = wire::read_frames_until_ready(&mut reader).await;
        assert_eq!(wire::error_code(&committed), None);
        assert_eq!(committed.last().map(|frame| frame.1.as_slice()), Some(&b"I"[..]));
        wire::write_frames(&mut writer, vec![wire::parse_frame("excess", "SELECT * FROM public.records r(a,b,c)"),
            wire::describe_statement_frame("excess"), wire::sync_frame()]).await;
        let excess = wire::read_frames_until_ready(&mut reader).await;
        assert_eq!(wire::error_code(&excess).as_deref(), Some("42P10"));
        drop(socket);
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("cleanup");
}
