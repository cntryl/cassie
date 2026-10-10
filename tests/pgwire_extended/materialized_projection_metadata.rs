use std::net::SocketAddr;
use std::time::Duration;

use super::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

type WireCycle = Vec<(u8, Vec<u8>)>;

async fn run_metadata_probe(addr: SocketAddr, password: &str) -> Vec<WireCycle> {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut writer) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    wire::complete_startup_with_password(&mut reader, &mut writer, password).await;
    let mut cycles = Vec::new();
    for query in [
        "SELECT n,note FROM projection_type_offset ORDER BY n",
        "SELECT n,note FROM projection_type_all ORDER BY n",
    ] {
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
        cycles
            .push(wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await);
    }
    drop(socket);
    cycles
}

fn assert_metadata_cycle(
    frames: &WireCycle,
    expected_tags: &[u8],
    expected_rows: &[Vec<Option<String>>],
    expected_command: &[u8],
) {
    assert_eq!(
        frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        expected_tags,
        "complete direct projection cycle: {frames:?}"
    );
    assert_eq!(wire::error_code(frames), None);
    assert_eq!(
        wire::parse_row_description(&frames[0].1),
        vec![
            wire::RowDescription {
                name: "n".into(),
                table_oid: 0,
                attr_num: 0,
                type_oid: 20,
                type_size: 8,
                type_mod: -1,
                format_code: 0,
            },
            wire::RowDescription {
                name: "note".into(),
                table_oid: 0,
                attr_num: 0,
                type_oid: 25,
                type_size: -1,
                type_mod: -1,
                format_code: 0,
            },
        ]
    );
    assert_eq!(wire::data_rows(frames).as_slice(), expected_rows);
    assert_eq!(frames[frames.len() - 2].1, expected_command);
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

#[test]
fn should_report_declared_type_oids_for_direct_projection_wire_rows() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("direct_projection_type_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        let password = uuid::Uuid::new_v4().to_string();
        config.password = password.clone();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("root", None);
        for statement in [
            "CREATE TABLE projection_type_source (n BIGINT,note TEXT)",
            "INSERT INTO projection_type_source VALUES (3,NULL),(1,'first'),(2,'second')",
            "CREATE MATERIALIZED PROJECTION projection_type_offset WITH (analytical = true) AS SELECT n,note FROM projection_type_source ORDER BY n OFFSET 1",
            "CREATE MATERIALIZED PROJECTION projection_type_all WITH (analytical = true) AS SELECT n,note FROM projection_type_source ORDER BY n",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .expect(statement);
        }
        let server = wire::spawn_server(cassie.clone()).await;

        // Act
        let cycles = run_metadata_probe(server.addr, &password).await;
        server.stop().await;
        drop(session);
        drop(cassie);

        // Assert
        assert_eq!(cycles.len(), 2);
        assert_metadata_cycle(
            &cycles[0],
            b"TDDCZ",
            &[
                vec![Some("2".into()), Some("second".into())],
                vec![Some("3".into()), None],
            ],
            b"SELECT 2\0",
        );
        assert_metadata_cycle(
            &cycles[1],
            b"TDDDCZ",
            &[
                vec![Some("1".into()), Some("first".into())],
                vec![Some("2".into()), Some("second".into())],
                vec![Some("3".into()), None],
            ],
            b"SELECT 3\0",
        );
    });
    drop(runtime);
    std::fs::remove_dir_all(path).expect("remove fixture");
}
