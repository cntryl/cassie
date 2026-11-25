use super::*;

async fn copy_in_frames(
    server: &CompatibilityServer,
    sql: &str,
    data: &[u8],
) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let (response_tag, response_payload) = support_pgwire::read_wire_frame(&mut reader).await;
    if response_tag == b'G' {
        support_pgwire::write_frames(
            &mut writer,
            vec![
                support_pgwire::copy_data_frame(data),
                support_pgwire::copy_done_frame(),
            ],
        )
        .await;
    }
    let mut frames =
        support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    frames.insert(0, (response_tag, response_payload));
    frames
}

fn copy_command(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

include!("forms/nulls.rs");

include!("forms/resources.rs");

include!("forms/semantics.rs");
