use super::*;
use std::time::Duration;

#[test]
fn should_filter_copy_from_stdin_rows_with_copy_where() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_where").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_where (id INT, label TEXT)")
            .await
            .expect("create table for COPY WHERE");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_where (id, label) FROM STDIN \
                 WITH (FORMAT csv) WHERE id > 1",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;
        let (mut copy_frames, copied_command) = if copy_response_tag == b'G' {
            support_pgwire::write_frames(
                &mut writer,
                vec![
                    support_pgwire::copy_data_frame(b"1,skip\n2,keep\n"),
                    support_pgwire::copy_done_frame(),
                ],
            )
            .await;
            let frames = support_pgwire::read_frames_until_ready_within(
                &mut reader,
                Duration::from_secs(5),
            )
            .await;
            let command = frames
                .iter()
                .find(|(tag, _)| *tag == b'C')
                .map(|(_, payload)| {
                    String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload))
                        .into_owned()
                });
            (frames, command)
        } else {
            (Vec::new(), None)
        };
        copy_frames.insert(
            0,
            (copy_response_tag, copy_response_payload.clone()),
        );
        let rows = client
            .query(
                "SELECT id, label FROM compat_copy_where ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after COPY WHERE");

        // Assert
        let error_code = support_pgwire::error_code(&copy_frames);
        drop((reader, writer, client));
        server.shutdown(connection).await;

        assert_eq!(
            copy_response_tag,
            b'G',
            "COPY WHERE should enter COPY IN; payload: {copy_response_payload:?}; SQLSTATE: {error_code:?}"
        );
        assert_eq!(copied_command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 2);
        assert_eq!(rows[0].get::<_, String>(1), "keep");
    });
}

#[test]
fn should_match_copy_csv_header_to_target_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_header_match").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_copy_header_match (id INT, label TEXT)")
            .await
            .expect("create table for COPY HEADER MATCH");

        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire COPY client");
        let (mut reader, mut writer) = socket.into_split();
        support_pgwire::complete_startup(&mut reader, &mut writer).await;

        // Act
        support_pgwire::write_frames(
            &mut writer,
            vec![support_pgwire::simple_query_frame(
                "COPY compat_copy_header_match (id, label) FROM STDIN \
                 WITH (FORMAT csv, HEADER MATCH)",
            )],
        )
        .await;
        let (copy_response_tag, copy_response_payload) =
            support_pgwire::read_wire_frame(&mut reader).await;
        let (mut copy_frames, copied_command) = if copy_response_tag == b'G' {
            support_pgwire::write_frames(
                &mut writer,
                vec![
                    support_pgwire::copy_data_frame(b"id,label\n1,keep\n"),
                    support_pgwire::copy_done_frame(),
                ],
            )
            .await;
            let frames =
                support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5))
                    .await;
            let command = frames
                .iter()
                .find(|(tag, _)| *tag == b'C')
                .map(|(_, payload)| {
                    String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload))
                        .into_owned()
                });
            (frames, command)
        } else {
            (Vec::new(), None)
        };
        copy_frames.insert(0, (copy_response_tag, copy_response_payload.clone()));
        let rows = client
            .query(
                "SELECT id, label FROM compat_copy_header_match ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after COPY HEADER MATCH");

        // Assert
        let error_code = support_pgwire::error_code(&copy_frames);
        drop((reader, writer, client));
        server.shutdown(connection).await;

        assert_eq!(
            copy_response_tag, b'G',
            "COPY HEADER MATCH should enter COPY IN; payload: {copy_response_payload:?}; \
             SQLSTATE: {error_code:?}"
        );
        assert_eq!(copied_command.as_deref(), Some("COPY 1"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "keep");
    });
}
