use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

struct ProbeOutput {
    success: bool,
    timed_out: bool,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct CompatibilityServer {
    addr: SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl CompatibilityServer {
    async fn start() -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("construct Cassie");
        cassie.startup().expect("start Cassie");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let _ =
                cassie::pgwire::server::run_with_listener(listener, Arc::new(cassie), config).await;
        });
        Self { addr, server }
    }

    async fn shutdown(self) {
        self.server.abort();
        let _ = self.server.await;
    }
}

fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn sqlx probe: {error}"))?;
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("poll sqlx probe: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect sqlx probe output: {error}"))?;
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
                .map_err(|error| format!("collect timed-out sqlx probe output: {error}"))?;
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
#[ignore = "requires the pinned external sqlx fixture"]
fn should_validate_sqlx_read_model_probe_when_enabled() {
    // Arrange
    if std::env::var("CASSIE_RUN_SQLX_COMPAT").ok().as_deref() != Some("1") {
        eprintln!("set CASSIE_RUN_SQLX_COMPAT=1 to run the optional sqlx probe");
        return;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start().await;
        let connection = format!(
            "postgresql://root:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );

        // Act
        let output = tokio::task::spawn_blocking(move || {
            let mut command = Command::new("cargo");
            command.args([
                "run",
                "--quiet",
                "--locked",
                "--manifest-path",
                "tests/fixtures/sqlx_probe/Cargo.toml",
                "--",
                &connection,
            ]);
            run_external_probe(command, Duration::from_secs(120))
        })
        .await
        .expect("sqlx probe blocking task should complete")
        .expect("run sqlx compatibility probe");
        server.shutdown().await;

        // Assert
        assert!(
            !output.timed_out,
            "sqlx probe timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(
            output.success,
            "sqlx probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status_code, output.stdout, output.stderr
        );
        let expected = [
            "sqlx_catalog=compat_sqlx_probe",
            "sqlx_prepared=alpha",
            "sqlx_transaction_row_count=1",
            "sqlx_duplicate_sqlstate=23505",
            "sqlx_missing_sqlstate=42P01",
            "sqlx_joined=alpha",
            "sqlx_after_duplicate=alpha",
            "sqlx_after_missing=alpha",
            "sqlx_same_session=true",
        ];
        for marker in &expected {
            assert!(
                output.stdout.lines().any(|line| line == *marker),
                "missing SQLx probe result: {marker}"
            );
        }
        for marker in expected {
            println!("{marker}");
        }
    });
}
