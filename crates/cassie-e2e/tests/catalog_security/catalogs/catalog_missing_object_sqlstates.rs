use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

use super::support_pgwire::{
    complete_startup, data_dir, parse_error_fields, read_wire_frame, simple_query_frame,
    spawn_server,
};

async fn query_sqlstate(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    sql: &str,
) -> Option<String> {
    tokio::io::AsyncWriteExt::write_all(writer, &simple_query_frame(sql))
        .await
        .expect("write query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush query");
    let mut sqlstate = None;
    loop {
        let (tag, payload) = read_wire_frame(reader).await;
        match tag {
            b'E' => {
                sqlstate = parse_error_fields(&payload)
                    .into_iter()
                    .find(|(field, _)| *field == 'C')
                    .map(|(_, value)| value);
            }
            b'Z' => return sqlstate,
            _ => {}
        }
    }
}

async fn sqlstates_for(label: &str, setup: &[&str], statements: &[&str]) -> Vec<Option<String>> {
    let path = data_dir(label);
    let config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("startup");
    let server = spawn_server(cassie).await;
    let mut socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut write_half) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    complete_startup(&mut reader, &mut write_half).await;
    for sql in setup {
        assert_eq!(
            query_sqlstate(&mut reader, &mut write_half, sql).await,
            None,
            "setup statement failed: {sql}"
        );
    }
    let mut sqlstates = Vec::with_capacity(statements.len());
    for sql in statements {
        sqlstates.push(query_sqlstate(&mut reader, &mut write_half, sql).await);
    }
    drop(socket);
    server.stop().await;
    let _ = std::fs::remove_dir_all(path);
    sqlstates
}

#[test]
fn should_report_undefined_table_sqlstate_for_projection_maintenance_on_missing_objects() {
    // Arrange
    let statements = [
        "VERIFY PROJECTION missing_maintenance_target",
        "DIFF PROJECTION missing_maintenance_target WITH missing_maintenance_other",
        "COMPARE PROJECTION missing_maintenance_target WITH MANIFEST '{\"root_digest\":\"abc\"}'",
        "PLAN REPAIR PROJECTION missing_maintenance_target SCOPE range",
        "REPAIR PROJECTION missing_maintenance_target SCOPE full-rebuild",
    ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let sqlstates = runtime.block_on(sqlstates_for(
        "missing_projection_maintenance",
        &[],
        &statements,
    ));

    // Assert
    assert_eq!(sqlstates, vec![Some("42P01".to_string()); statements.len()]);
}

#[test]
fn should_report_undefined_object_sqlstate_for_projection_maintenance_on_missing_version() {
    // Arrange
    let setup = [
            "CREATE TABLE missing_version_docs (title TEXT)",
            "INSERT INTO missing_version_docs (title) VALUES ('alpha')",
            "CREATE MATERIALIZED PROJECTION missing_version_projection AS SELECT title FROM missing_version_docs",
        ];
    let statements = [
            "VERIFY PROJECTION missing_version_projection VERSION v9",
            "DIFF PROJECTION missing_version_projection VERSION v9 WITH missing_version_projection",
            "COMPARE PROJECTION missing_version_projection VERSION v9 WITH MANIFEST '{\"root_digest\":\"abc\"}'",
            "PLAN REPAIR PROJECTION missing_version_projection VERSION v9 SCOPE range",
            "REPAIR PROJECTION missing_version_projection VERSION v9 SCOPE full-rebuild",
        ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let sqlstates = runtime.block_on(sqlstates_for(
        "missing_projection_version",
        &setup,
        &statements,
    ));

    // Assert
    assert_eq!(sqlstates, vec![Some("42704".to_string()); statements.len()]);
}

#[test]
fn should_report_undefined_table_sqlstate_when_serial_default_sequence_is_missing() {
    // Arrange
    let setup = [
        "CREATE TABLE public.missing_sequence_orders (order_no SERIAL, label TEXT)",
        "DROP SEQUENCE public.missing_sequence_orders_order_no_seq",
    ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let sqlstates = runtime.block_on(sqlstates_for(
        "missing_default_sequence",
        &setup,
        &["INSERT INTO missing_sequence_orders (label) VALUES ('one')"],
    ));

    // Assert
    assert_eq!(sqlstates, vec![Some("42P01".to_string())]);
}

#[test]
fn should_report_invalid_schema_sqlstate_when_creating_in_a_missing_schema() {
    // Arrange
    let setup = [
        "CREATE SCHEMA existing_target",
        "CREATE TABLE public.ghost_source (id INT)",
    ];
    let statements = [
        "CREATE TABLE ghost.t (id BIGINT)",
        "CREATE VIEW ghost.v AS SELECT id FROM public.ghost_source",
        "CREATE SEQUENCE ghost.s",
        "CREATE TABLE existing_target.t (id BIGINT)",
        "CREATE TABLE public.t (id BIGINT)",
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'ghost'",
    ];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let sqlstates = runtime.block_on(sqlstates_for(
        "create_in_missing_schema",
        &setup,
        &statements,
    ));

    // Assert
    assert_eq!(
        sqlstates,
        vec![
            Some("3F000".to_string()),
            Some("3F000".to_string()),
            Some("3F000".to_string()),
            None,
            None,
            None,
        ]
    );
}
