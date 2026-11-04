//! Internal CTAS creation stays inside its enclosing wire transaction.

use crate::support_pgwire as wire;
use std::time::Duration;

fn ctas_cycles(succeed: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        for extended in [false, true] {
            let connection = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connection");
            let (mut reader, mut writer) = connection.into_split();
            wire::complete_startup(&mut reader, &mut writer).await;
            let name = format!("wire_ctas_{extended}_{succeed}");
            let expression = if succeed { "1" } else { "1 / 0" };
            let sql = format!("CREATE TABLE {name} AS SELECT {expression} AS n");
            let frames = if extended {
                vec![
                    wire::parse_frame("ctas", &sql),
                    wire::bind_frame("", "ctas", &[]),
                    wire::execute_frame(""),
                    wire::sync_frame(),
                ]
            } else {
                vec![wire::simple_query_frame(&sql)]
            };

            // Act
            wire::write_frames(&mut writer, frames).await;
            let result =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
            let inspect = if succeed {
                format!("SELECT n FROM {name}")
            } else {
                format!("SELECT count(*) FROM pg_class WHERE relname = '{name}'")
            };
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(&inspect)]).await;
            let observed =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;

            // Assert
            assert_eq!(
                result.last().map(|(_, body)| body.as_slice()),
                Some(&b"I"[..])
            );
            if succeed {
                assert_eq!(wire::error_code(&result), None);
                assert_eq!(
                    result
                        .iter()
                        .find(|(tag, _)| *tag == b'C')
                        .map(|(_, body)| body.as_slice()),
                    Some(&b"SELECT 1\0"[..])
                );
            } else {
                assert_eq!(wire::error_code(&result).as_deref(), Some("22012"));
                assert!(result.iter().all(|(tag, _)| *tag != b'C'));
            }
            assert_eq!(wire::error_code(&observed), None);
            let expected = if succeed { "1" } else { "0" };
            assert_eq!(
                wire::data_rows(&observed),
                vec![vec![Some(expected.to_owned())]]
            );
        }
        server.stop().await;
    });
}

#[test]
fn should_rollback_failed_ctas_in_both_wire_protocols() {
    // Arrange
    let succeed = false;
    // Act
    ctas_cycles(succeed);
    // Assert
    // Each actual protocol must return 22012, ReadyForQuery(I) and no relation.
}

#[test]
fn should_publish_successful_ctas_in_both_wire_protocols() {
    // Arrange
    let succeed = true;
    // Act
    ctas_cycles(succeed);
    // Assert
    // Each protocol must expose exactly the committed row after completion.
}
