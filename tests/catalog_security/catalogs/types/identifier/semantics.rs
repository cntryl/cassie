

#[test]
fn should_match_builtin_regcollation_identity() {
    // Arrange
    let fixture = sql_fixture("issue-818-regcollation-builtin-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT 'pg_catalog.\"default\"'::regcollation::oid, \
                    'pg_catalog.\"default\"'::regcollation::text, \
                    pg_typeof('pg_catalog.\"default\"'::regcollation)::oid, \
                    'pg_catalog.\"default\"'::regcollation::oid = \
                      to_regcollation('pg_catalog.\"default\"')::oid, \
                    to_regcollation('missing_collation') IS NULL, \
                    'default'::regcollation::oid = \
                      'pg_catalog.\"default\"'::regcollation::oid",
        )
        .expect("resolve and format the built-in default collation identity");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(100),
            Value::String("\"default\"".to_owned()),
            Value::Int64(4_191),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_encode_regcollation_identity_on_wire() {
    // Arrange
    let path = wire::data_dir("issue-818-regcollation-wire-identity");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let sql = "SELECT 'pg_catalog.\"default\"'::regcollation AS collation_id";

    // Act
    let batches = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let mut batches = Vec::new();
        for format in [0_i16, 1_i16] {
            let result_format_code = [format];
            let result_formats: &[i16] = if format == 0 {
                &[]
            } else {
                &result_format_code
            };
            let cycle = vec![
                wire::parse_frame("", sql),
                wire::describe_statement_frame(""),
                wire::bind_frame_with_formats("", "", &[], &[], result_formats),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ];
            wire::write_frames(&mut writer, cycle).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });
    let _ = std::fs::remove_dir_all(&path);

    // Assert
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0_i16, 1_i16].into_iter().zip(&batches) {
        assert_eq!(wire::error_code(frames), None);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, payload)| wire::parse_row_description(payload))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.iter().enumerate() {
            assert_eq!(columns.len(), 1);
            assert_eq!(columns[0].name, "collation_id");
            assert_eq!(
                (
                    columns[0].type_oid,
                    columns[0].type_size,
                    columns[0].type_mod
                ),
                (4_191, 4, -1)
            );
            assert_eq!(columns[0].format_code, if index == 0 { 0 } else { format });
        }
        if format == 0 {
            assert_eq!(
                wire::data_rows(frames),
                vec![vec![Some("\"default\"".to_owned())]]
            );
        } else {
            let data_row = frames
                .iter()
                .find(|(tag, _)| *tag == b'D')
                .map(|(_, payload)| payload.as_slice())
                .expect("binary DataRow");
            assert_eq!(data_row, &[0, 1, 0, 0, 0, 4, 0, 0, 0, 100]);
        }
    }
}
