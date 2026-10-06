//! Preparation-only ownership at accepted Simple Query ready boundaries.

use super::support_pgwire as wire;
use super::support_pgwire_transaction_boundary::{copy_access_fixture, run_script, Prefix};

#[test]
fn should_finish_preparation_only_portals_on_idle_simple_query_completion() {
    // Arrange
    let cases = [
        ("", None, b"12IZ".as_slice()),
        ("SELECT FROM", Some("42601"), b"12EZ".as_slice()),
        ("SELECT 'unterminated", Some("42601"), b"12EZ".as_slice()),
    ];
    for (sql, error, tags) in cases {
        let boundary = vec![
            wire::parse_frame("named_s", "SELECT 9"),
            wire::bind_frame("named_p", "named_s", &[]),
            wire::flush_frame(),
            wire::simple_query_frame(sql),
        ];
        let tail = vec![
            vec![wire::execute_frame("named_p"), wire::sync_frame()],
            vec![
                wire::bind_frame("fresh_p", "named_s", &[]),
                wire::execute_frame("fresh_p"),
                wire::sync_frame(),
            ],
        ];

        // Act
        let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

        // Assert
        assert_eq!(
            result
                .boundary
                .iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            tags
        );
        assert_eq!(wire::error_code(&result.boundary).as_deref(), error);
        assert_eq!(
            result.boundary.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(wire::error_code(&result.tail[0]).as_deref(), Some("26000"));
        assert_eq!(wire::error_code(&result.tail[1]), None);
        assert_eq!(
            wire::data_rows(&result.tail[1]),
            vec![vec![Some("9".to_string())]]
        );
    }
}

#[test]
fn should_finish_preparation_only_portals_on_error_recovery_sync() {
    // Arrange
    let boundary = vec![
        wire::parse_frame("named_s", "SELECT 9"),
        wire::bind_frame("named_p", "named_s", &[]),
        wire::parse_frame("bad_s", "SELECT FROM"),
        wire::sync_frame(),
    ];
    let tail = vec![
        vec![wire::execute_frame("named_p"), wire::sync_frame()],
        vec![
            wire::bind_frame("fresh_p", "named_s", &[]),
            wire::execute_frame("fresh_p"),
            wire::sync_frame(),
        ],
    ];

    // Act
    let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"12EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("42601"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&result.tail[0]).as_deref(), Some("26000"));
    assert_eq!(wire::error_code(&result.tail[1]), None);
    assert_eq!(
        wire::data_rows(&result.tail[1]),
        vec![vec![Some("9".to_string())]]
    );
}

#[test]
fn should_roll_back_implicit_read_ownership_on_revoked_copy_access() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("implicit-copy-revoked-access");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (cassie, admin) = copy_access_fixture(&path);
        let server = wire::spawn_server(cassie.clone()).await;
        let owner = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("owner");
        let (mut reader, mut writer) = tokio::io::split(owner);
        wire::complete_startup_as(
            &mut reader,
            &mut writer,
            "boundary_reader",
            "postgres",
            "reader-secret",
        )
        .await;
        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame("named_s", "SELECT 9"),
                wire::bind_frame("named_p", "named_s", &[]),
                wire::execute_frame("named_p"),
                wire::flush_frame(),
            ],
        )
        .await;
        let prefix = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut frames = Vec::new();
            for _ in 0..5 {
                frames.push(wire::read_wire_frame(&mut reader).await);
            }
            frames
        })
        .await
        .expect("execution barrier before revocation");
        assert_eq!(
            prefix.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"12TDC"
        );
        cassie
            .execute_sql(
                &admin,
                "REVOKE CONNECT ON DATABASE postgres FROM boundary_reader",
                Vec::new(),
            )
            .expect("revoke access after execution");

        // Act
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "COPY wire_copy_denied FROM STDIN WITH (FORMAT csv)",
            )],
        )
        .await;
        let failed =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE postgres TO boundary_reader",
                Vec::new(),
            )
            .expect("restore access");
        wire::write_frames(
            &mut writer,
            vec![wire::execute_frame("named_p"), wire::sync_frame()],
        )
        .await;
        let stale =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 9")]).await;
        let recovered =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;

        // Assert
        assert_eq!(
            failed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"EZ"
        );
        assert_eq!(wire::error_code(&failed).as_deref(), Some("42501"));
        assert_eq!(
            failed.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(wire::error_code(&stale).as_deref(), Some("26000"));
        assert_eq!(wire::error_code(&recovered), None);
        assert_eq!(
            wire::data_rows(&recovered),
            vec![vec![Some("9".to_string())]]
        );
        drop((reader, writer));
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_finish_unsynced_implicit_work_on_simple_commit_or_rollback() {
    // Arrange
    for (control, count) in [("COMMIT", "1"), ("ROLLBACK", "0")] {
        let boundary = vec![wire::simple_query_frame(control)];
        let tail = vec![
            vec![wire::execute_frame("prefix_p"), wire::sync_frame()],
            vec![wire::simple_query_frame("SELECT 9")],
        ];

        // Act
        let result = run_script(
            Prefix::Implicit,
            boundary,
            "SELECT COUNT(*) FROM wire_tx",
            tail,
        );

        // Assert
        assert_eq!(
            wire::data_rows(&result.before),
            vec![vec![Some("0".to_owned())]]
        );
        assert_eq!(
            result
                .boundary
                .iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            b"CZ"
        );
        assert_eq!(result.boundary[0].1, format!("{control}\0").as_bytes());
        assert_eq!(result.boundary[1].1, b"I");
        assert_eq!(
            wire::data_rows(&result.after),
            vec![vec![Some(count.to_owned())]]
        );
        assert_eq!(wire::error_code(&result.tail[0]).as_deref(), Some("26000"));
        assert_eq!(wire::error_code(&result.tail[1]), None);
        assert_eq!(
            wire::data_rows(&result.tail[1]),
            vec![vec![Some("9".to_owned())]]
        );
        assert_eq!(
            wire::data_rows(&result.terminal),
            vec![vec![Some(count.to_owned())]]
        );
    }
}

#[test]
fn should_roll_back_only_the_implicit_segment_after_an_explicit_commit() {
    // Arrange
    let boundary = vec![wire::simple_query_frame(
        "BEGIN; INSERT INTO wire_tx (n) VALUES (1); COMMIT; INSERT INTO wire_tx (n) VALUES (2); SELECT n FROM wire_tx WHERE (n / 0) = 1; INSERT INTO wire_tx (n) VALUES (3)",
    )];

    // Act
    let result = run_script(
        Prefix::Idle,
        boundary,
        "SELECT n FROM wire_tx ORDER BY n",
        Vec::new(),
    );

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"CCCCEZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("22012"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_release_unsynced_implicit_ownership_on_disconnect() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("implicit-disconnect-ownership");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let fixture = cassie.create_session("tester", None);
        cassie
            .execute_sql(&fixture, "CREATE TABLE wire_tx (n INT)", Vec::new())
            .expect("fixture");
        let server = wire::spawn_server(cassie.clone()).await;
        let owner = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("owner");
        let (mut reader, mut writer) = tokio::io::split(owner);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame("named_s", "INSERT INTO wire_tx (n) VALUES (1)"),
                wire::bind_frame("named_p", "named_s", &[]),
                wire::execute_frame("named_p"),
                wire::flush_frame(),
            ],
        )
        .await;
        let prefix = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            let mut frames = Vec::new();
            for _ in 0..3 {
                frames.push(wire::read_wire_frame(&mut reader).await);
            }
            frames
        })
        .await
        .expect("unsynced execution barrier");
        assert_eq!(
            prefix.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"12C"
        );
        assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(1));

        // Act
        drop((reader, writer));
        let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let metrics = cassie.metrics();
                if metrics["pgwire"]["active_sessions"].as_u64() == Some(0) {
                    break metrics;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("bounded disconnect cleanup");
        let observer = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("observer");
        let (mut reader, mut writer) = tokio::io::split(observer);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT COUNT(*) FROM wire_tx")],
        )
        .await;
        let rows =
            wire::read_frames_until_ready_within(&mut reader, std::time::Duration::from_secs(5))
                .await;

        // Assert
        assert_eq!(released["pgwire"]["prepared_statements"].as_u64(), Some(0));
        assert_eq!(released["pgwire"]["portals"].as_u64(), Some(0));
        assert_eq!(
            released["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        assert_eq!(
            released["runtime"]["active_operator_workers"].as_u64(),
            Some(0)
        );
        assert_eq!(wire::error_code(&rows), None);
        assert_eq!(wire::data_rows(&rows), vec![vec![Some("0".to_owned())]]);
        drop((reader, writer));
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_the_original_session_snapshot_when_promoting_implicit_work() {
    // Arrange
    let boundary = vec![
        wire::parse_frame(
            "setting_s",
            "SELECT set_config('application_name', 'inside', false)",
        ),
        wire::bind_frame("setting_p", "setting_s", &[]),
        wire::execute_frame("setting_p"),
        wire::simple_query_frame("BEGIN"),
    ];
    let tail = vec![
        vec![wire::sync_frame()],
        vec![wire::simple_query_frame("ROLLBACK")],
        vec![wire::simple_query_frame(
            "SELECT current_setting('application_name')",
        )],
    ];

    // Act
    let result = run_script(Prefix::Idle, boundary, "SELECT COUNT(*) FROM wire_tx", tail);

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        wire::data_rows(&result.boundary),
        vec![vec![Some("inside".to_owned())]]
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(result.tail[0], vec![(b'Z', b"T".to_vec())]);
    assert_eq!(
        result.tail[1].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&result.tail[2]), None);
    assert_eq!(
        wire::data_rows(&result.tail[2]),
        vec![vec![Some(String::new())]]
    );
}

#[test]
fn should_finish_failed_implicit_commit_at_the_joined_query_boundary() {
    // Arrange
    let boundary = vec![wire::simple_query_frame("COMMIT")];
    let tail = vec![
        vec![wire::simple_query_frame("SELECT 9")],
        vec![wire::sync_frame()],
    ];

    // Act
    let result = run_script(
        Prefix::ImplicitParentConflict,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("23503"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_owned())]]
    );
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_owned())]]
    );
    assert_eq!(result.tail[1], vec![(b'Z', b"I".to_vec())]);
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_owned())]]
    );
}

#[test]
fn should_fail_explicit_transactions_when_extended_preparation_fails() {
    // Arrange
    let cases = [
        (wire::parse_frame("bad_s", "SELECT FROM"), "42601"),
        (wire::bind_frame("bad_p", "missing_s", &[]), "26000"),
        (wire::describe_statement_frame("missing_s"), "26000"),
    ];
    for (frame, code) in cases {
        let boundary = vec![frame, wire::sync_frame()];
        let tail = vec![
            vec![wire::execute_frame("prefix_p"), wire::sync_frame()],
            vec![wire::simple_query_frame("COMMIT")],
            vec![wire::simple_query_frame("ROLLBACK")],
            vec![wire::simple_query_frame("SELECT 9")],
        ];

        // Act
        let result = run_script(
            Prefix::Explicit,
            boundary,
            "SELECT COUNT(*) FROM wire_tx",
            tail,
        );

        // Assert
        assert_eq!(
            result
                .boundary
                .iter()
                .map(|frame| frame.0)
                .collect::<Vec<_>>(),
            b"EZ"
        );
        assert_eq!(wire::error_code(&result.boundary).as_deref(), Some(code));
        assert_eq!(
            result.boundary.last().map(|frame| frame.1.as_slice()),
            Some(&b"E"[..])
        );
        assert_eq!(
            wire::data_rows(&result.after),
            vec![vec![Some("0".to_owned())]]
        );
        assert_eq!(wire::error_code(&result.tail[0]).as_deref(), Some("22000"));
        assert_eq!(
            result.tail[0].last().map(|frame| frame.1.as_slice()),
            Some(&b"E"[..])
        );
        assert_eq!(
            result.tail[2].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(wire::error_code(&result.tail[1]).as_deref(), Some("22000"));
        assert_eq!(wire::error_code(&result.tail[3]), None);
        assert_eq!(
            wire::data_rows(&result.tail[3]),
            vec![vec![Some("9".to_owned())]]
        );
        assert_eq!(
            wire::data_rows(&result.terminal),
            vec![vec![Some("0".to_owned())]]
        );
    }
}

#[test]
fn should_roll_back_implicit_work_when_extended_frame_decoding_fails() {
    // Arrange
    // Execute frame with a complete frame boundary but no required portal/limit payload.
    let boundary = vec![vec![b'E', 0, 0, 0, 4], wire::sync_frame()];
    let tail = vec![
        vec![wire::simple_query_frame("SELECT 9")],
        vec![wire::sync_frame()],
    ];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("08P01"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_owned())]]
    );
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_owned())]]
    );
    assert_eq!(result.tail[1], vec![(b'Z', b"I".to_vec())]);
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_owned())]]
    );
}

#[test]
fn should_roll_back_implicit_work_when_simple_frame_decoding_fails() {
    // Arrange
    // Query payload is completely framed but lacks the required terminating NUL.
    let boundary = vec![vec![b'Q', 0, 0, 0, 5, b'x']];
    let tail = vec![
        vec![wire::simple_query_frame("SELECT 9")],
        vec![wire::sync_frame()],
    ];

    // Act
    let result = run_script(
        Prefix::Implicit,
        boundary,
        "SELECT COUNT(*) FROM wire_tx",
        tail,
    );

    // Assert
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(wire::error_code(&result.boundary).as_deref(), Some("08P01"));
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        vec![vec![Some("0".to_owned())]]
    );
    assert_eq!(wire::error_code(&result.tail[0]), None);
    assert_eq!(
        wire::data_rows(&result.tail[0]),
        vec![vec![Some("9".to_owned())]]
    );
    assert_eq!(result.tail[1], vec![(b'Z', b"I".to_vec())]);
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("0".to_owned())]]
    );
}
