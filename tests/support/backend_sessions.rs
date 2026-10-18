//! Two real pgwire sessions for catalog statistics and SQL signaling contracts.

use std::ops::AsyncFnOnce;
use std::time::Duration;

use cassie::app::Cassie;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

use crate::support_pgwire as wire;

pub type Frames = Vec<(u8, Vec<u8>)>;

pub struct Session {
    pub reader: ReadHalf<TcpStream>,
    pub writer: WriteHalf<TcpStream>,
    pub pid: i32,
}

impl Session {
    async fn connect(server: &wire::PgwireServer) -> Self {
        let socket = TcpStream::connect(server.addr)
            .await
            .expect("connect backend session");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (pid, _) = wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
        Self {
            reader,
            writer,
            pid,
        }
    }

    pub async fn send(&mut self, sql: &str) {
        wire::write_frames(&mut self.writer, vec![wire::simple_query_frame(sql)]).await;
    }

    pub async fn read(&mut self) -> Frames {
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    pub async fn query(&mut self, sql: &str) -> Frames {
        self.send(sql).await;
        self.read().await
    }
}

pub struct Sessions {
    pub admin: Session,
    pub target: Session,
    server: wire::PgwireServer,
}

struct Directory(String);

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn with_sessions<F, R>(label: &str, body: F) -> R
where
    F: AsyncFnOnce(&mut Sessions) -> R,
{
    wire::use_local_storage();
    // Declared before the runtime: on panic, all server tasks and storage
    // owners are dropped before the fixture directory is removed.
    let directory = Directory(wire::data_dir(label));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("Cassie");
        cassie.startup().expect("startup backend fixture");
        let server = wire::spawn_server(cassie).await;
        let admin = Session::connect(&server).await;
        let target = Session::connect(&server).await;
        let mut sessions = Sessions {
            admin,
            target,
            server,
        };
        let result = body(&mut sessions).await;
        let Sessions {
            admin,
            target,
            server,
        } = sessions;
        drop(admin);
        drop(target);
        server.stop().await;
        result
    })
}

pub fn assert_rows(frames: &[(u8, Vec<u8>)], expected: &[&[&str]]) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let expected = expected
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| Some((*value).to_owned()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(wire::data_rows(frames), expected);
}
