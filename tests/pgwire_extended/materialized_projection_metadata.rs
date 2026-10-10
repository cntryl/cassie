use std::net::SocketAddr;

use super::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

async fn run_metadata_probe(addr: SocketAddr) -> Vec<Vec<wire::RowDescription>> {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut writer) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    wire::complete_startup(&mut reader, &mut writer).await;
    let mut descriptions = Vec::new();
    for query in [
        "SELECT n,note FROM projection_type_offset ORDER BY n",
        "SELECT n,note FROM projection_type_all ORDER BY n",
    ] {
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
        let frames = wire::read_frames_until_ready(&mut reader).await;
        let description = frames.iter().find(|(tag, _)| *tag == b'T').map_or_else(
            || panic!("RowDescription for {query}: {frames:?}"),
            |(_, payload)| wire::parse_row_description(payload),
        );
        descriptions.push(description);
    }
    drop(socket);
    descriptions
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
        config.password = "postgres".to_owned();
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
        let descriptions = run_metadata_probe(server.addr).await;
        server.stop().await;
        drop(session);
        drop(cassie);

        // Assert
        for description in descriptions {
            assert_eq!(
                description
                    .iter()
                    .map(|column| (column.name.as_str(), column.type_oid, column.type_size, column.type_mod, column.format_code))
                    .collect::<Vec<_>>(),
                vec![("n", 20, 8, -1, 0), ("note", 25, -1, -1, 0)]
            );
        }
    });
    drop(runtime);
    std::fs::remove_dir_all(path).expect("remove fixture");
}
