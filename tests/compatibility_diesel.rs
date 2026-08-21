use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
#[path = "support/sql.rs"]
mod support;
use support::data_dir;

struct ProbeOutput {
    success: bool,
    timed_out: bool,
    status_code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct CompatibilityServer {
    data_dir: String,
    addr: SocketAddr,
    server: tokio::task::JoinHandle<()>,
}

impl CompatibilityServer {
    async fn start() -> Self {
        std::env::set_var("CASSIE_STORAGE_MODE", "memory");
        let data_dir = data_dir("diesel_probe");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&data_dir, config.clone())
            .expect("construct Cassie");
        cassie.startup().expect("start Cassie");

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

    async fn shutdown(self) {
        self.server.abort();
        let _ = self.server.await;
        let _ = std::fs::remove_dir_all(self.data_dir);
    }
}

fn run_external_probe(mut command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn Diesel probe: {error}"))?;
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| format!("poll Diesel probe: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("collect Diesel probe output: {error}"))?;
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
                .map_err(|error| format!("collect timed-out Diesel probe output: {error}"))?;
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
#[ignore = "requires the pinned external Diesel fixture"]
fn should_validate_diesel_read_model_probe_when_enabled() {
    // Arrange
    if std::env::var("CASSIE_RUN_DIESEL_COMPAT").ok().as_deref() != Some("1") {
        eprintln!("set CASSIE_RUN_DIESEL_COMPAT=1 to run the optional Diesel probe");
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
                "tests/fixtures/diesel_probe/Cargo.toml",
                "--",
                &connection,
            ]);
            run_external_probe(command, Duration::from_secs(120))
        })
        .await
        .expect("Diesel probe blocking task should complete")
        .expect("run Diesel compatibility probe");
        server.shutdown().await;

        // Assert
        assert!(
            !output.timed_out,
            "Diesel probe timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout, output.stderr
        );
        assert!(
            output.success,
            "Diesel probe failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status_code, output.stdout, output.stderr
        );
        assert!(output.stdout.contains("diesel_catalog=compat_diesel_probe"));
        assert!(output.stdout.contains("diesel_prepared=alpha"));
        assert!(output.stdout.contains("diesel_transaction_row_count=1"));
        assert!(output
            .stdout
            .contains("diesel_duplicate_error=unique_violation"));
        assert!(output
            .stdout
            .contains("diesel_missing_error=relation_missing"));
    });
}
