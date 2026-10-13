use super::super::compatibility_matrix::{run_external_probe, CompatibilityServer, ProbeOutput};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use uuid::Uuid;

struct ArtifactDir(std::path::PathBuf);

impl Drop for ArtifactDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct RestoreProbe {
    output: Result<ProbeOutput, String>,
    rows: Option<Result<Vec<(i32, String)>, String>>,
}

struct DumpRestoreProbe {
    dump_version: Result<ProbeOutput, String>,
    restore_version: Result<ProbeOutput, String>,
    dump: Result<ProbeOutput, String>,
    restore: Option<RestoreProbe>,
}

async fn run_probe(command: Command, timeout: Duration) -> Result<ProbeOutput, String> {
    tokio::task::spawn_blocking(move || run_external_probe(command, timeout))
        .await
        .map_err(|error| format!("external probe task failed: {error}"))?
}

async fn probe_version(binary: &str) -> Result<ProbeOutput, String> {
    let mut command = Command::new(binary);
    command.arg("--version");
    run_probe(command, Duration::from_secs(5)).await
}

async fn run_pg_dump(pg_dump_bin: &str, dump_path: &Path) -> Result<ProbeOutput, String> {
    let server = CompatibilityServer::start("pg_dump_source").await;
    let (client, connection) = server.connect().await;
    client
        .batch_execute(
            "CREATE TABLE compat_pg18_dump_items (id INTEGER PRIMARY KEY, payload TEXT NOT NULL)",
        )
        .await
        .expect("create dump fixture table");
    client
        .batch_execute("INSERT INTO compat_pg18_dump_items VALUES (1, 'first'), (2, 'second')")
        .await
        .expect("populate dump fixture");
    drop(client);

    let connection_url = format!(
        "postgresql://root:postgres@127.0.0.1:{}/postgres",
        server.addr.port()
    );
    let mut command = Command::new(pg_dump_bin);
    command
        .args([
            "--format=custom",
            "--no-owner",
            "--no-privileges",
            "--file",
            dump_path.to_str().expect("dump path should be UTF-8"),
            "--dbname",
            &connection_url,
        ])
        .env("PGPASSWORD", "postgres")
        .env("PGCONNECT_TIMEOUT", "5");
    let dump = run_probe(command, Duration::from_secs(30)).await;
    server.shutdown(connection).await;
    dump
}

async fn run_pg_restore(pg_restore_bin: &str, dump_path: &Path) -> RestoreProbe {
    let target = CompatibilityServer::start("pg_dump_target").await;
    let (target_client, target_connection) = target.connect().await;
    let target_url = format!(
        "postgresql://root:postgres@127.0.0.1:{}/postgres",
        target.addr.port()
    );
    let mut command = Command::new(pg_restore_bin);
    command
        .args([
            "--no-owner",
            "--no-privileges",
            "--exit-on-error",
            "--dbname",
            &target_url,
            dump_path.to_str().expect("dump path should be UTF-8"),
        ])
        .env("PGPASSWORD", "postgres")
        .env("PGCONNECT_TIMEOUT", "5");
    let output = run_probe(command, Duration::from_secs(30)).await;
    let rows = if output
        .as_ref()
        .is_ok_and(|probe| probe.success && !probe.timed_out)
    {
        Some(
            target_client
                .query(
                    "SELECT id, payload FROM compat_pg18_dump_items ORDER BY id",
                    &[],
                )
                .await
                .map(|rows| {
                    rows.into_iter()
                        .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                        .collect::<Vec<_>>()
                })
                .map_err(|error| error.to_string()),
        )
    } else {
        None
    };
    target.shutdown(target_connection).await;
    RestoreProbe { output, rows }
}

async fn run_dump_restore_probe(
    pg_dump_bin: &str,
    pg_restore_bin: &str,
    dump_path: &Path,
) -> DumpRestoreProbe {
    let dump_version = probe_version(pg_dump_bin).await;
    let restore_version = probe_version(pg_restore_bin).await;
    let dump = run_pg_dump(pg_dump_bin, dump_path).await;
    let restore = if dump
        .as_ref()
        .is_ok_and(|probe| probe.success && !probe.timed_out)
    {
        Some(run_pg_restore(pg_restore_bin, dump_path).await)
    } else {
        None
    };
    DumpRestoreProbe {
        dump_version,
        restore_version,
        dump,
        restore,
    }
}

fn assert_version(binary: &str, version: Result<ProbeOutput, String>) {
    let version = version.unwrap_or_else(|error| panic!("{binary} version probe failed: {error}"));
    assert!(version.success, "{binary} --version: {}", version.stderr);
    assert!(
        version.stdout.contains("18.6"),
        "expected pinned {binary} 18.6, got {}",
        version.stdout
    );
}

fn assert_probe_succeeded(binary: &str, probe: &ProbeOutput) {
    assert!(
        !probe.timed_out,
        "{binary} timed out\nstdout:\n{}\nstderr:\n{}",
        probe.stdout, probe.stderr
    );
    assert!(
        probe.success,
        "{binary} failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        probe.status_code, probe.stdout, probe.stderr
    );
}

#[test]
#[ignore = "requires pinned PostgreSQL 18.6 client utilities; run with CASSIE_RUN_PG_DUMP_COMPAT=1 cargo test --locked --test compatibility compatibility::pg_dump_restore::should_round_trip_a_postgresql_18_6_dump_through_pgwire -- --ignored --nocapture"]
fn should_round_trip_a_postgresql_18_6_dump_through_pgwire() {
    // Arrange
    if std::env::var("CASSIE_RUN_PG_DUMP_COMPAT").ok().as_deref() != Some("1") {
        eprintln!("set CASSIE_RUN_PG_DUMP_COMPAT=1 to run the PostgreSQL 18.6 dump probe");
        return;
    }
    let pg_dump_bin = std::env::var("CASSIE_PG_DUMP_BIN").unwrap_or_else(|_| "pg_dump".into());
    let pg_restore_bin =
        std::env::var("CASSIE_PG_RESTORE_BIN").unwrap_or_else(|_| "pg_restore".into());
    let artifact_dir = std::env::temp_dir().join(format!("cassie-pg18-dump-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&artifact_dir).expect("create dump artifact directory");
    let _artifact_dir = ArtifactDir(artifact_dir.clone());
    let dump_path = artifact_dir.join("fixture.dump");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let probe = runtime.block_on(run_dump_restore_probe(
        &pg_dump_bin,
        &pg_restore_bin,
        &dump_path,
    ));

    // Assert
    assert_version("pg_dump", probe.dump_version);
    assert_version("pg_restore", probe.restore_version);
    let dump = probe.dump.expect("pg_dump should run");
    assert_probe_succeeded("pg_dump", &dump);
    let restore = probe.restore.expect("successful dump should be restored");
    let restore_output = restore.output.expect("pg_restore should run");
    assert_probe_succeeded("pg_restore", &restore_output);
    assert_eq!(
        restore
            .rows
            .expect("successful restore should query rows")
            .expect("restored fixture query should succeed"),
        vec![(1, "first".to_string()), (2, "second".to_string())]
    );
}
