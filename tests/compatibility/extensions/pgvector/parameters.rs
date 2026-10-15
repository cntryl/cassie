use super::*;
use std::time::Duration;

#[test]
fn should_preserve_vector_values_given_text_parameters_when_the_statement_executes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, _) = start_vector_server("pgvector_text_parameter").await;
        let statement = client
            .prepare("SELECT $1::text::vector::text")
            .await
            .expect("prepare a text parameter cast into vector");

        // Act
        let result = client
            .query_one(&statement, &[&"[1,2,3]"])
            .await
            .expect("convert the text parameter through the vector input function");

        // Assert
        assert_eq!(statement.params()[0].oid(), 25);
        assert_eq!(result.get::<_, String>(0), "[1,2,3]");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_vector_values_given_binary_parameters_when_binary_results_are_requested() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, vector_oid) =
            start_vector_server("pgvector_binary_parameter").await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect a raw pgwire client for binary vector formats");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut vector = Vec::with_capacity(16);
        vector.extend_from_slice(&3_i16.to_be_bytes());
        vector.extend_from_slice(&0_i16.to_be_bytes());
        for value in [1.0_f32, 2.0, 3.0] {
            vector.extend_from_slice(&value.to_be_bytes());
        }
        let frames = vec![
            wire::parse_frame_with_types("vector_binary", "SELECT $1", &[vector_oid]),
            wire::bind_frame_with_formats(
                "",
                "vector_binary",
                &[1],
                &[Some(vector.as_slice())],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ];

        // Act
        wire::write_frames(&mut writer, frames).await;
        let responses =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        // Assert
        assert_eq!(wire::error_code(&responses), None);
        let description = responses
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("binary vector query should return RowDescription");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].type_oid, vector_oid);
        assert_eq!(columns[0].format_code, 1);
        let row = responses
            .iter()
            .find(|(tag, _)| *tag == b'D')
            .expect("binary vector query should return a DataRow");
        let payload = &row.1;
        assert_eq!(
            i16::from_be_bytes(payload[0..2].try_into().expect("field count")),
            1
        );
        let length = i32::from_be_bytes(payload[2..6].try_into().expect("field length"));
        assert_eq!(length, 16);
        assert_eq!(&payload[6..], vector.as_slice());
        drop(reader);
        drop(writer);
        drop(client);
        server.shutdown(connection).await;
    });
}
