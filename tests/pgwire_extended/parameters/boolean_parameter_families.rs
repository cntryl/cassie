use cassie::app::Cassie;

use super::support_pgwire as support;

type WireFrame = (u8, Vec<u8>);

fn run_flag_queries(label: &str, binds: &[(&str, i32, &str)]) -> Vec<Vec<WireFrame>> {
    support::use_local_storage();
    let path = support::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE bool_param_docs (id INT, flag BOOLEAN)",
            "INSERT INTO bool_param_docs (id, flag) VALUES (1, true), (2, false)",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed boolean table");
        }
        let server = support::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        support::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for (sql, oid, value) in binds {
            support::write_frames(
                &mut writer,
                vec![
                    support::parse_frame_with_types("", sql, &[*oid]),
                    support::bind_frame("", "", &[value]),
                    support::execute_frame(""),
                    support::sync_frame(),
                ],
            )
            .await;
            batches.push(support::read_frames_until_ready(&mut reader).await);
        }
        server.stop().await;
        batches
    });
    let _ = std::fs::remove_dir_all(path);
    batches
}

#[test]
fn should_reject_non_boolean_declared_parameters_against_boolean_columns() {
    // Arrange
    let flag = "SELECT id FROM bool_param_docs WHERE flag = $1 ORDER BY id";
    let reversed = "SELECT id FROM bool_param_docs WHERE $1 = flag ORDER BY id";
    let binds = [
        (flag, 23, "2"),
        (flag, 20, "7"),
        (flag, 701, "0.5"),
        (flag, 25, "true"),
        (reversed, 23, "1"),
        (flag, 16, "true"),
        (flag, 0, "false"),
    ];

    // Act
    let batches = run_flag_queries("bool-param-families", &binds);

    // Assert
    for frames in &batches[..5] {
        assert!(support::error_code(frames).is_some());
        assert_eq!(
            support::data_rows(frames),
            [] as [std::vec::Vec<std::option::Option<std::string::String>>; 0]
        );
    }
    assert_eq!(
        support::data_rows(&batches[5]),
        vec![vec![Some("1".to_string())]]
    );
    assert_eq!(
        support::data_rows(&batches[6]),
        vec![vec![Some("2".to_string())]]
    );
}
