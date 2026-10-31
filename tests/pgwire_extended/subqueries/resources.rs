fn assert_scalar_result_limit_contract(
    setup: &[Vec<(u8, Vec<u8>)>],
    baseline: &[(u8, Vec<u8>)],
    within_limit: &[(u8, Vec<u8>)],
    overflow: &[(u8, Vec<u8>)],
    recovery: &[(u8, Vec<u8>)],
) {
    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar result-limit setup ErrorResponses: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(baseline),
        None,
        "plain projection limit control ErrorResponse: {:?}",
        baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(baseline),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(
        wire::error_code(within_limit),
        None,
        "within-limit scalar-subquery ErrorResponse: {:?}",
        within_limit
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(within_limit),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(
        wire::error_code(overflow).as_deref(),
        Some("54000"),
        "plain projection overflow control ErrorResponse: {:?}",
        overflow
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(overflow), Vec::<Vec<Option<String>>>::new());
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("scalar result-limit ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message == "query result row limit exceeded: 3 > 2"
    }));
    assert_eq!(
        wire::error_code(recovery),
        None,
        "post-limit recovery ErrorResponse: {:?}",
        recovery
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("46".to_owned())]]);
}

#[test]
fn should_project_scalar_subqueries_within_the_configured_result_row_limit() {
    // Arrange
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.max_result_rows = 2;
    let storage = sql_fixture_with_config("issue-762-scalar-result-limit", &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start scalar result-limit fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (setup, baseline, within_limit, overflow, recovery) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(storage.cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut setup = Vec::new();
        for statement in [
            "CREATE TABLE issue_762_cap_outer (id INT4 PRIMARY KEY)",
            "CREATE TABLE issue_762_cap_inner (id INT4, value INT4)",
            "INSERT INTO issue_762_cap_outer VALUES (1), (2), (3)",
            "INSERT INTO issue_762_cap_inner VALUES (1, 10), (2, 20), (3, 30)",
        ] {
            wire::write_frames(
                &mut writer,
                vec![wire::simple_query_frame(statement)],
            )
            .await;
            setup.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, o.id * 10 AS scalar_value \
                 FROM issue_762_cap_outer o WHERE o.id <= 2 ORDER BY o.id",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, (SELECT i.value FROM issue_762_cap_inner i WHERE i.id = o.id) AS scalar_value \
                 FROM issue_762_cap_outer o WHERE o.id <= 2 ORDER BY o.id",
            )],
        )
        .await;
        let within_limit =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, o.id * 10 AS scalar_value \
                 FROM issue_762_cap_outer o ORDER BY o.id",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 46 AS recovery")])
            .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (setup, baseline, within_limit, overflow, recovery)
    });
    drop(runtime);

    // Assert
    assert_scalar_result_limit_contract(&setup, &baseline, &within_limit, &overflow, &recovery);
}
