use super::*;

type WireFrame = (u8, Vec<u8>);

async fn read_until_first_data_row(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<WireFrame>,
    frames: &mut Vec<WireFrame>,
) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(frame) = receiver.recv().await else {
                return false;
            };
            let tag = frame.0;
            frames.push(frame);
            if tag == b'D' {
                return true;
            }
            if tag == b'E' || tag == b'Z' {
                return false;
            }
        }
    })
    .await
    .unwrap_or(false)
}

async fn read_until_ready(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<WireFrame>,
    frames: &mut Vec<WireFrame>,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(frame) = receiver.recv().await else {
                return;
            };
            let ready = frame.0 == b'Z';
            frames.push(frame);
            if ready {
                return;
            }
        }
    })
    .await
    .expect("the pgwire command should finish with ReadyForQuery");
}

async fn send_cancel(address: std::net::SocketAddr, process_id: i32, secret_key: i32) {
    let mut cancel_socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect the CancelRequest socket");
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("send CancelRequest after observing the streamed row");
    cancel_socket
        .shutdown()
        .await
        .expect("close the CancelRequest socket");
}
