use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use uuid::Uuid;

const SQLALCHEMY_PROBE: &str = r#"
import sys

from sqlalchemy import create_engine, text
from sqlalchemy.exc import DBAPIError

url = sys.argv[1]
print("sqlalchemy_step=create_engine", flush=True)
engine = create_engine(url, future=True, use_native_hstore=False)

print("sqlalchemy_step=connect", flush=True)
with engine.connect() as conn:
    print("sqlalchemy_step=ddl", flush=True)
    conn.exec_driver_sql(
        "CREATE TABLE compat_sqlalchemy_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)"
    )
    conn.execute(
        text("INSERT INTO compat_sqlalchemy_probe (id, title) VALUES (:id, :title)"),
        {"id": 1, "title": "alpha"},
    )
    conn.commit()

    print("sqlalchemy_step=catalog", flush=True)
    catalog = conn.execute(
        text(
            "SELECT table_name FROM information_schema.tables "
            "WHERE table_name = :table_name"
        ),
        {"table_name": "compat_sqlalchemy_probe"},
    ).scalar_one()
    print("sqlalchemy_step=simple", flush=True)
    simple = conn.exec_driver_sql(
        "SELECT title FROM compat_sqlalchemy_probe ORDER BY title"
    ).scalar_one()
    print("sqlalchemy_step=prepared", flush=True)
    prepared = conn.execute(
        text("SELECT title FROM compat_sqlalchemy_probe WHERE id = :id"),
        {"id": 1},
    ).scalar_one()

    print("sqlalchemy_step=duplicate_error", flush=True)
    try:
        conn.execute(
            text("INSERT INTO compat_sqlalchemy_probe (id, title) VALUES (:id, :title)"),
            {"id": 2, "title": "alpha"},
        )
        conn.commit()
    except DBAPIError as exc:
        conn.rollback()
        duplicate_sqlstate = getattr(getattr(exc, "orig", None), "sqlstate", "")
    else:
        raise AssertionError("duplicate unique insert succeeded")

    print("sqlalchemy_step=missing_relation_error", flush=True)
    try:
        conn.execute(text("SELECT title FROM compat_sqlalchemy_missing")).all()
    except DBAPIError as exc:
        conn.rollback()
        missing_sqlstate = getattr(getattr(exc, "orig", None), "sqlstate", "")
    else:
        raise AssertionError("missing relation query succeeded")

print(f"sqlalchemy_catalog={catalog}")
print(f"sqlalchemy_simple={simple}")
print(f"sqlalchemy_prepared={prepared}")
print(f"sqlalchemy_duplicate_sqlstate={duplicate_sqlstate}")
print(f"sqlalchemy_missing_sqlstate={missing_sqlstate}")
"#;

struct ProbeOutput {
    success: bool,
    timed_out: bool,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-sqlalchemy-compatibility-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

struct CompatibilityServer {
    data_dir: String,
    addr: SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl CompatibilityServer {
    async fn start(label: &str) -> Self {
        with_fallback();
        let data_dir = data_dir(label);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone()).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run(addr.to_string(), Arc::new(cassie.clone()), config)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        Self {
            data_dir,
            addr,
            server,
        }
    }

    async fn shutdown_without_client(self) {
        self.server.abort();
        let _ = self.server.await;
        let _ = std::fs::remove_dir_all(self.data_dir);
    }
}

fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn external probe: {error}"))?;
    let started = Instant::now();

    loop {
        if child
            .try_wait()
            .map_err(|error| format!("poll external probe: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect external probe output: {error}"))?;
            return Ok(ProbeOutput {
                success: output.status.success(),
                timed_out: false,
                status_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect timed-out external probe output: {error}"))?;
            return Ok(ProbeOutput {
                success: false,
                timed_out: true,
                status_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires external SQLAlchemy compatibility harness"]
fn should_validate_sqlalchemy_read_model_probe_when_enabled() {
    // Arrange
    if std::env::var("CASSIE_RUN_SQLALCHEMY_COMPAT")
        .ok()
        .as_deref()
        != Some("1")
    {
        eprintln!("set CASSIE_RUN_SQLALCHEMY_COMPAT=1 to run the optional SQLAlchemy probe");
        return;
    }
    let python_bin =
        std::env::var("CASSIE_SQLALCHEMY_PYTHON").unwrap_or_else(|_| "python3".to_string());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("sqlalchemy_probe").await;
        let connection = format!(
            "postgresql+psycopg://postgres:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );

        // Act
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(python_bin);
            command.args(["-c", SQLALCHEMY_PROBE, &connection]);
            run_external_probe(command, Duration::from_secs(20))
        })
        .await
        .expect("SQLAlchemy probe blocking task should complete");
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                server.shutdown_without_client().await;
                panic!("run SQLAlchemy compatibility probe: {error}");
            }
        };
        server.shutdown_without_client().await;

        // Assert
        assert!(
            !output.timed_out,
            "SQLAlchemy probe timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(
            output.success,
            "SQLAlchemy probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status_code, output.stdout, output.stderr
        );
        assert!(output
            .stdout
            .contains("sqlalchemy_catalog=compat_sqlalchemy_probe"));
        assert!(output.stdout.contains("sqlalchemy_simple=alpha"));
        assert!(output.stdout.contains("sqlalchemy_prepared=alpha"));
        assert!(output
            .stdout
            .contains("sqlalchemy_duplicate_sqlstate=23505"));
        assert!(output.stdout.contains("sqlalchemy_missing_sqlstate=42P01"));
    });
}
