use super::*;

async fn copy_to_stdout(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire COPY client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let frames =
        support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    drop((reader, writer));
    frames
}

fn copy_payload(frames: &[(u8, Vec<u8>)]) -> Vec<u8> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'd')
        .flat_map(|(_, payload)| payload.iter().copied())
        .collect()
}

fn copy_command(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

include!("missing_forms/binary.rs");

include!("missing_forms/semantics.rs");
