#[test]
fn should_follow_search_path_for_regproc_families() {
    // Arrange
    let fixture = shadowed_routine_fixture("issue-818-regproc-search-path");
    let expected = vec![vec![
        Value::Bool(true),
        Value::Bool(true),
        Value::Bool(true),
        Value::Bool(true),
    ]];

    // Act
    fixture
        .execute("SET search_path TO issue_818_reg_lookup_a, issue_818_reg_lookup_b, pg_catalog")
        .expect("put the first routine schema first");
    let first_path = fixture
        .execute(
            "SELECT 'wave_lookup'::regproc::oid = \
                    'issue_818_reg_lookup_a.wave_lookup'::regproc::oid, \
                    to_regproc('wave_lookup')::oid = \
                    'issue_818_reg_lookup_a.wave_lookup'::regproc::oid, \
                    'wave_lookup(integer)'::regprocedure::oid = \
                    'issue_818_reg_lookup_a.wave_lookup(integer)'::regprocedure::oid, \
                    to_regprocedure('wave_lookup(integer)')::oid = \
                    'issue_818_reg_lookup_a.wave_lookup(integer)'::regprocedure::oid",
        )
        .expect("resolve regproc types from the first search-path schema");
    fixture
        .execute("SET search_path TO issue_818_reg_lookup_b, issue_818_reg_lookup_a, pg_catalog")
        .expect("put the second routine schema first");
    let second_path = fixture
        .execute(
            "SELECT 'wave_lookup'::regproc::oid = \
                    'issue_818_reg_lookup_b.wave_lookup'::regproc::oid, \
                    to_regproc('wave_lookup')::oid = \
                    'issue_818_reg_lookup_b.wave_lookup'::regproc::oid, \
                    'wave_lookup(integer)'::regprocedure::oid = \
                    'issue_818_reg_lookup_b.wave_lookup(integer)'::regprocedure::oid, \
                    to_regprocedure('wave_lookup(integer)')::oid = \
                    'issue_818_reg_lookup_b.wave_lookup(integer)'::regprocedure::oid",
        )
        .expect("resolve regproc types from the second search-path schema");

    // Assert
    assert_eq!(first_path.rows(), expected);
    assert_eq!(second_path.rows(), expected);
}

#[test]
fn should_skip_inaccessible_schema_for_regproc_families() {
    // Arrange
    let fixture = shadowed_routine_fixture("issue-818-regproc-schema-usage");
    fixture
        .execute("CREATE ROLE issue_818_reg_lookup_reader")
        .expect("create restricted reader role");
    fixture
        .execute(
            "REVOKE ALL ON SCHEMA issue_818_reg_lookup_a, issue_818_reg_lookup_b \
             FROM PUBLIC",
        )
        .expect("remove public schema access");
    fixture
        .execute("GRANT USAGE ON SCHEMA issue_818_reg_lookup_b TO issue_818_reg_lookup_reader")
        .expect("grant the second schema to the reader");
    let reader = fixture
        .cassie
        .create_session("issue_818_reg_lookup_reader", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "SET search_path TO issue_818_reg_lookup_a, issue_818_reg_lookup_b, pg_catalog",
            vec![],
        )
        .expect("set the restricted reader's search path");

    // Act
    let selected = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT 'wave_lookup'::regproc::oid = \
                    'issue_818_reg_lookup_b.wave_lookup'::regproc::oid, \
                    to_regproc('wave_lookup')::oid = \
                    'issue_818_reg_lookup_b.wave_lookup'::regproc::oid, \
                    'wave_lookup(integer)'::regprocedure::oid = \
                    'issue_818_reg_lookup_b.wave_lookup(integer)'::regprocedure::oid, \
                    to_regprocedure('wave_lookup(integer)')::oid = \
                    'issue_818_reg_lookup_b.wave_lookup(integer)'::regprocedure::oid",
            vec![],
        )
        .expect("resolve routines through the accessible schema");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

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
    wire::use_local_storage();
    let path = wire::data_dir("issue-818-regcollation-wire-identity");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let sql = "SELECT 'pg_catalog.\"default\"'::regcollation AS collation_id";

    // Act
    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
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
