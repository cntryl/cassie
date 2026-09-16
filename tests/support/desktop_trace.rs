use std::collections::HashSet;
use std::fmt::Write;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};
use uuid::Uuid;

const TRACE_SCHEMA_VERSION: u32 = 1;
const PGADMIN_VERSION: &str = "9.16";
const PGADMIN_UPSTREAM_REVISION: &str = "888a053231923e6704b56165cf248db056252144";
const PGADMIN_FIXTURE_SHA256: &str =
    "9b6eae4dafd118128dd96e6c5fe9f339f139eb9b1e18d809f77f1ba98e82759f";
const DBEAVER_VERSION: &str = "26.1.3";
const DBEAVER_UPSTREAM_REVISION: &str = "af12125f8cedfc9fd857416facf356ad69d29b04";
const DBEAVER_FIXTURE_SHA256: &str =
    "b025a5d4d259b12806319cf8c65ef026f30ad347a21678bf705474971a4e8990";
const POSTGRES_JDBC_VERSION: &str = "42.7.11";
const REQUIRED_WORKFLOWS: [&str; 7] = [
    "initialization",
    "navigator",
    "properties",
    "query",
    "transaction",
    "plan",
    "grid-edit",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopClient {
    PgAdmin,
    Dbeaver,
}

impl DesktopClient {
    fn fixture_name(self) -> &'static str {
        match self {
            Self::PgAdmin => "pgadmin",
            Self::Dbeaver => "dbeaver",
        }
    }

    fn fixture_sha256(self) -> &'static str {
        match self {
            Self::PgAdmin => PGADMIN_FIXTURE_SHA256,
            Self::Dbeaver => DBEAVER_FIXTURE_SHA256,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopTrace {
    schema_version: u32,
    client: ClientIdentity,
    steps: Vec<TraceStep>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientIdentity {
    name: String,
    version: String,
    driver_version: Option<String>,
    upstream_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceStep {
    id: String,
    workflow: String,
    protocol_mode: ProtocolMode,
    sql: String,
    expected_columns: Vec<String>,
    expected_row_count: usize,
    expected_sqlstate: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ProtocolMode {
    SimpleQuery,
    ExtendedQuery,
}

pub fn parse_trace(source: &str) -> Result<DesktopTrace, String> {
    let trace = serde_json::from_str::<DesktopTrace>(source)
        .map_err(|error| format!("desktop trace must be valid JSON: {error}"))?;
    validate_shape(&trace)?;
    Ok(trace)
}

pub fn parse_pinned_trace(
    source: &str,
    expected_client: DesktopClient,
) -> Result<DesktopTrace, String> {
    let mut observed_digest = String::with_capacity(64);
    for byte in Sha256::digest(source.as_bytes()) {
        write!(&mut observed_digest, "{byte:02x}").expect("write fixture digest");
    }
    if observed_digest != expected_client.fixture_sha256() {
        return Err(format!(
            "desktop trace fixture digest does not match pinned {} fixture",
            expected_client.fixture_name()
        ));
    }
    let trace = parse_trace(source)?;
    validate_trace_for(&trace, expected_client)?;
    Ok(trace)
}

pub fn validate_trace_for(trace: &DesktopTrace, expected: DesktopClient) -> Result<(), String> {
    let (version, driver, revision) = match expected {
        DesktopClient::PgAdmin => (PGADMIN_VERSION, None, PGADMIN_UPSTREAM_REVISION),
        DesktopClient::Dbeaver => (
            DBEAVER_VERSION,
            Some(POSTGRES_JDBC_VERSION),
            DBEAVER_UPSTREAM_REVISION,
        ),
    };
    if trace.client.name != expected.fixture_name()
        || trace.client.version != version
        || trace.client.driver_version.as_deref() != driver
        || trace.client.upstream_revision != revision
    {
        return Err(format!(
            "desktop trace client identity does not match pinned {} {}",
            expected.fixture_name(),
            version
        ));
    }
    Ok(())
}

fn validate_shape(trace: &DesktopTrace) -> Result<(), String> {
    if trace.schema_version != TRACE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported desktop trace schema version {}",
            trace.schema_version
        ));
    }
    if trace.steps.is_empty() {
        return Err("desktop trace must contain workflow steps".to_string());
    }
    let mut ids = HashSet::new();
    let mut workflows = HashSet::new();
    for step in &trace.steps {
        if step.id.is_empty() || !ids.insert(step.id.as_str()) {
            return Err("desktop trace step IDs must be non-empty and unique".to_string());
        }
        if step.sql.is_empty() || step.sql.len() > 1024 * 1024 {
            return Err(format!("desktop trace step '{}' has invalid SQL", step.id));
        }
        if step.expected_columns.iter().any(String::is_empty) {
            return Err(format!(
                "desktop trace step '{}' has an empty expected column",
                step.id
            ));
        }
        if !REQUIRED_WORKFLOWS.contains(&step.workflow.as_str()) {
            return Err(format!(
                "desktop trace step '{}' has unsupported workflow '{}'",
                step.id, step.workflow
            ));
        }
        if let Some(sqlstate) = &step.expected_sqlstate {
            if sqlstate.len() != 5
                || !sqlstate
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            {
                return Err(format!(
                    "desktop trace step '{}' has an invalid SQLSTATE",
                    step.id
                ));
            }
            if !step.expected_columns.is_empty() || step.expected_row_count != 0 {
                return Err(format!(
                    "desktop trace step '{}' has an invalid error expectation",
                    step.id
                ));
            }
        }
        workflows.insert(step.workflow.as_str());
    }
    let missing = REQUIRED_WORKFLOWS
        .into_iter()
        .filter(|workflow| !workflows.contains(workflow))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "desktop trace is missing required workflow steps: {}",
            missing.join(", ")
        ));
    }
    Ok(())
}

pub async fn replay_trace(trace: &DesktopTrace) -> Result<(), String> {
    let client_kind = match trace.client.name.as_str() {
        "pgadmin" => DesktopClient::PgAdmin,
        "dbeaver" => DesktopClient::Dbeaver,
        other => return Err(format!("unsupported desktop trace client '{other}'")),
    };
    validate_trace_for(trace, client_kind)?;
    let server = TraceServer::start(client_kind.fixture_name()).await?;
    let result = replay_steps(&server.client, &trace.steps).await;
    server.shutdown().await;
    result
}

async fn replay_steps(client: &Client, steps: &[TraceStep]) -> Result<(), String> {
    for step in steps {
        let observed = match step.protocol_mode {
            ProtocolMode::SimpleQuery => execute_simple(client, step).await,
            ProtocolMode::ExtendedQuery => execute_extended(client, step).await,
        };
        match (&step.expected_sqlstate, observed) {
            (Some(expected), Err(error)) => {
                let actual = error
                    .as_db_error()
                    .map(|db_error| db_error.code().code())
                    .ok_or_else(|| format!("step '{}' returned no SQLSTATE", step.id))?;
                if actual != expected {
                    return Err(format!(
                        "step '{}' expected SQLSTATE {} but observed {}",
                        step.id, expected, actual
                    ));
                }
            }
            (Some(expected), Ok(_)) => {
                return Err(format!(
                    "step '{}' expected SQLSTATE {} but succeeded",
                    step.id, expected
                ));
            }
            (None, Err(error)) => return Err(format!("step '{}' failed: {error}", step.id)),
            (None, Ok((columns, row_count))) => {
                if columns != step.expected_columns || row_count != step.expected_row_count {
                    return Err(format!(
                        "step '{}' expected columns {:?} and {} rows but observed {:?} and {} rows",
                        step.id, step.expected_columns, step.expected_row_count, columns, row_count
                    ));
                }
            }
        }
    }
    Ok(())
}

async fn execute_simple(
    client: &Client,
    step: &TraceStep,
) -> Result<(Vec<String>, usize), tokio_postgres::Error> {
    let messages = client.simple_query(&step.sql).await?;
    let rows = messages
        .iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .collect::<Vec<_>>();
    let columns = rows.first().map_or_else(Vec::new, |row| {
        row.columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect()
    });
    Ok((columns, rows.len()))
}

async fn execute_extended(
    client: &Client,
    step: &TraceStep,
) -> Result<(Vec<String>, usize), tokio_postgres::Error> {
    let statement = client.prepare(&step.sql).await?;
    if step.expected_columns.is_empty() {
        client.execute(&statement, &[]).await?;
        return Ok((Vec::new(), 0));
    }
    let columns = statement
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect::<Vec<_>>();
    let rows = client.query(&statement, &[]).await?;
    Ok((columns, rows.len()))
}

struct TraceServer {
    client: Client,
    connection: tokio::task::JoinHandle<()>,
    server: tokio::task::JoinHandle<()>,
    data_dir: String,
}

impl TraceServer {
    async fn start(label: &str) -> Result<Self, String> {
        let data_dir = std::env::temp_dir()
            .join(format!("cassie-desktop-trace-{label}-{}", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        let config = CassieRuntimeConfig {
            password: "postgres".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone())
            .map_err(|error| format!("construct trace server: {error}"))?;
        cassie
            .startup()
            .map_err(|error| format!("start trace server: {error}"))?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|error| format!("bind trace server: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("read trace server address: {error}"))?;
        drop(listener);
        let server_config = config;
        let server = tokio::spawn(async move {
            let _ =
                cassie::pgwire::server::run(address.to_string(), Arc::new(cassie), server_config)
                    .await;
        });
        let (client, connection) = connect(address).await?;
        Ok(Self {
            client,
            connection,
            server,
            data_dir,
        })
    }

    async fn shutdown(self) {
        drop(self.client);
        self.connection.abort();
        self.server.abort();
        let _ = self.connection.await;
        let _ = self.server.await;
        let _ = std::fs::remove_dir_all(self.data_dir);
    }
}

async fn connect(address: SocketAddr) -> Result<(Client, tokio::task::JoinHandle<()>), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(address.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        match config.connect(NoTls).await {
            Ok((client, connection)) => {
                let task = tokio::spawn(async move {
                    let _ = connection.await;
                });
                return Ok((client, task));
            }
            Err(error) if tokio::time::Instant::now() < deadline => {
                let _ = error;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(format!("connect trace client: {error}")),
        }
    }
}
