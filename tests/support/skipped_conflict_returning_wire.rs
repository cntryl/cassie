//! Wire transcripts for the finite skipped-conflict RETURNING qualification.

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

const INSERT: &str = "INSERT INTO skipped_returning (id,note) \
    VALUES (3,'third'),(2,'ignored'),(1,'first') \
    ON CONFLICT (id) DO NOTHING RETURNING id AS row_key,note";
const BOUND_INSERT: &str = "INSERT INTO skipped_returning (id,note) \
    VALUES ($1,$2),($3,$4),($5,$6) \
    ON CONFLICT (id) DO NOTHING RETURNING id AS row_key,note";
const OBSERVE: &str = "SELECT id,note FROM skipped_returning ORDER BY id";
pub type Frames = Vec<(u8, Vec<u8>)>;

pub struct QueryCounters {
    pub count: u64,
    pub rows: u64,
    pub errors: u64,
}

impl QueryCounters {
    fn read(fixture: &SqlFixture) -> Self {
        let snapshot = fixture.cassie.metrics();
        let query = &snapshot["query"];
        Self {
            count: query["count"].as_u64().expect("query success count"),
            rows: query["rows_returned_total"]
                .as_u64()
                .expect("query row count"),
            errors: query["errors_total"].as_u64().expect("query error count"),
        }
    }
}

pub struct Execution {
    pub frames: Frames,
    pub state: Frames,
    pub before: QueryCounters,
    pub after: QueryCounters,
}

pub struct Transcript {
    pub seed: Frames,
    pub parsed: Option<Frames>,
    pub first: Execution,
    pub retry: Execution,
    pub retired_state: Frames,
}

struct Client {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn connect(addr: SocketAddr) -> Self {
        let socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (reader, writer) = socket.into_split();
        let mut client = Self { reader, writer };
        wire::complete_startup(&mut client.reader, &mut client.writer).await;
        client
    }

    async fn cycle(&mut self, frames: Vec<Vec<u8>>) -> Frames {
        wire::write_frames(&mut self.writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }
}

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture.cassie.startup().expect("startup");
    for sql in [
        "CREATE TABLE skipped_returning (id BIGINT PRIMARY KEY,note TEXT)",
        "INSERT INTO skipped_returning (id,note) VALUES (2,'old')",
    ] {
        fixture.execute(sql).expect("seed conflict owner");
    }
    fixture
}

fn bind(portal: &str, binary: bool) -> Vec<u8> {
    let values = [(3_i64, "third"), (2, "ignored"), (1, "first")]
        .into_iter()
        .flat_map(|(id, note)| {
            let id = if binary {
                id.to_be_bytes().to_vec()
            } else {
                id.to_string().into_bytes()
            };
            [id, note.as_bytes().to_vec()]
        })
        .collect::<Vec<_>>();
    let parameters = values
        .iter()
        .map(|value| Some(value.as_slice()))
        .collect::<Vec<_>>();
    wire::bind_frame_with_formats(
        portal,
        "insert_rows",
        &[i16::from(binary)],
        &parameters,
        &[],
    )
}

async fn execute(
    fixture: &SqlFixture,
    client: &mut Client,
    observer: &mut Client,
    portal: &str,
    binary: Option<bool>,
) -> Execution {
    let request = binary.map_or_else(
        || vec![wire::simple_query_frame(INSERT)],
        |binary| {
            vec![
                bind(portal, binary),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]
        },
    );
    // Measure only this owner command, before the independent observer query.
    let before = QueryCounters::read(fixture);
    let frames = client.cycle(request).await;
    let after = QueryCounters::read(fixture);
    let state = observer
        .cycle(vec![wire::simple_query_frame(OBSERVE)])
        .await;
    Execution {
        frames,
        state,
        before,
        after,
    }
}

async fn execute_fixture(fixture: &SqlFixture, binary: Option<bool>) -> Transcript {
    let server = wire::spawn_server(fixture.cassie.clone()).await;
    let mut client = Client::connect(server.addr).await;
    let mut observer = Client::connect(server.addr).await;
    let seed = observer
        .cycle(vec![wire::simple_query_frame(OBSERVE)])
        .await;
    let parsed = if binary.is_some() {
        Some(
            client
                .cycle(vec![
                    wire::parse_frame_with_types(
                        "insert_rows",
                        BOUND_INSERT,
                        &[20, 25, 20, 25, 20, 25],
                    ),
                    wire::sync_frame(),
                ])
                .await,
        )
    } else {
        None
    };
    let first = execute(fixture, &mut client, &mut observer, "mixed_insert", binary).await;
    // A fresh Bind/new portal executes the statement again; never re-Execute first.
    let retry = execute(fixture, &mut client, &mut observer, "fresh_retry", binary).await;
    let pgwire = &fixture.cassie.metrics()["pgwire"];
    assert_eq!(
        pgwire["active_sessions"].as_u64().expect("active sessions"),
        2
    );
    let finished = pgwire["sessions_finished_total"]
        .as_u64()
        .expect("finished sessions");
    drop((client, observer));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let metrics = fixture.cassie.metrics();
            let pgwire = &metrics["pgwire"];
            if pgwire["active_sessions"].as_u64().expect("active sessions") == 0
                && pgwire["sessions_finished_total"]
                    .as_u64()
                    .expect("finished sessions")
                    == finished + 2
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both original connections must retire after acknowledged commands");
    let mut fresh_observer = Client::connect(server.addr).await;
    let retired_state = fresh_observer
        .cycle(vec![wire::simple_query_frame(OBSERVE)])
        .await;
    drop(fresh_observer);
    server.stop().await;
    Transcript {
        seed,
        parsed,
        first,
        retry,
        retired_state,
    }
}

pub fn run(label: &str, binary: Option<bool>) -> Transcript {
    let fixture = fixture(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let transcript = runtime.block_on(execute_fixture(&fixture, binary));
    // Detached connection owners retire before the strict SqlFixture directory guard.
    drop(runtime);
    drop(fixture);
    transcript
}
