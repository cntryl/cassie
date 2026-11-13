//! Fixtures for the named enum suites (#827): text-cell helpers over `SqlFixture`
//! and a pgwire run that exposes the allocated enum OIDs before frames are built.

use crate::support_sql_fixture::SqlFixture;
use crate::support_type_metadata_contract::{Cycle, Frames};
use cassie::app::Cassie;
use cassie::types::Value;

/// Renders one result cell as text, `None` for SQL NULL.
///
/// # Panics
///
/// Panics for cells that are not text, numeric, boolean or NULL; enum assertions
/// cast to `text` in SQL so a typed carrier never reaches this helper.
#[must_use]
pub fn cell_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Int64(number) => Some(number.to_string()),
        Value::Float64(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => panic!("expected a text-compatible cell"),
    }
}

/// The first column of every row as optional text, in result order.
///
/// # Panics
///
/// Panics when the statement fails or a row has no columns.
#[must_use]
pub fn texts(fixture: &SqlFixture, sql: &str) -> Vec<Option<String>> {
    fixture
        .rows(sql)
        .iter()
        .map(|row| cell_text(row.first().expect("a first column")))
        .collect()
}

/// The first column of every row as text, with NULL rendered as `NULL`.
#[must_use]
pub fn strings(fixture: &SqlFixture, sql: &str) -> Vec<String> {
    texts(fixture, sql)
        .into_iter()
        .map(|cell| cell.unwrap_or_else(|| "NULL".to_owned()))
        .collect()
}

/// The single cell of a one-row, one-column result.
///
/// # Panics
///
/// Panics when the result is not exactly one row.
#[must_use]
pub fn scalar(fixture: &SqlFixture, sql: &str) -> Option<String> {
    let mut cells = texts(fixture, sql);
    assert_eq!(cells.len(), 1, "expected one row for {sql}");
    cells.remove(0)
}

/// Runs `sql` on another session of the same instance.
///
/// # Errors
///
/// Returns the engine error for a failing statement.
pub fn execute_on(
    session: &cassie::app::CassieSession,
    fixture: &SqlFixture,
    sql: &str,
) -> Result<cassie::executor::QueryResult, cassie::app::CassieError> {
    fixture.cassie.execute_sql(session, sql, vec![])
}

/// Allocated identities of the fixture type named `state_code`.
#[derive(Debug, Clone, Copy)]
pub struct EnumOids {
    pub type_oid: i32,
    pub array_oid: i32,
}

/// Seeds an in-memory instance with `setup`, reads the OIDs of the type named
/// `state_code`, builds the cycles from them and exchanges the frames over an
/// owned, bound pgwire listener.
///
/// # Panics
///
/// Panics when setup, startup or the protocol exchange fails.
#[must_use]
pub fn run_with_enum_oids(
    label: &str,
    setup: &[&str],
    build_cycles: impl FnOnce(EnumOids) -> Vec<Cycle>,
) -> (EnumOids, Vec<Frames>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        if config.password.is_empty() {
            crate::support_sql::test_root_password().clone_into(&mut config.password);
        }
        let cassie = Cassie::new_in_memory_with_config(config)
            .unwrap_or_else(|error| panic!("create ephemeral fixture {label}: {error}"));
        cassie.startup().expect("startup before enum DDL");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .unwrap_or_else(|error| panic!("seed {label}: {sql}: {error}"));
        }
        let identity = cassie
            .execute_sql(
                &session,
                "SELECT oid::bigint, typarray::bigint FROM pg_catalog.pg_type \
                 WHERE typname = 'state_code'",
                Vec::new(),
            )
            .expect("read enum identities");
        let row = identity.rows().first().expect("one pg_type row").clone();
        let oid = |value: &Value| {
            i32::try_from(value.as_i64().expect("integer OID")).expect("OID fits i32")
        };
        let oids = EnumOids {
            type_oid: oid(&row[0]),
            array_oid: oid(&row[1]),
        };
        let cycles = build_cycles(oids);
        let server = crate::support_pgwire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        crate::support_pgwire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            crate::support_pgwire::write_frames(&mut writer, cycle).await;
            batches.push(
                crate::support_pgwire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        (oids, batches)
    })
}
