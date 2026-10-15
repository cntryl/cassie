use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::super::{run_external_probe, CompatibilityServer, ProbeOutput};
use super::{
    assert_prisma_aggregate_queries, assert_prisma_core_results, assert_prisma_create_query,
    assert_prisma_enum_queries, assert_prisma_introspection, assert_prisma_pagination_queries,
    assert_prisma_probe_completed, assert_prisma_query_event_inventory, assert_prisma_raw_queries,
    assert_prisma_rich_queries, assert_prisma_transaction_queries, assert_prisma_typed_results,
    assert_prisma_update_and_upsert_queries, parse_prisma_query_events, parse_prisma_result,
    read_package_version, PRISMA_PROBE, PRISMA_SCHEMA,
};
use tokio::runtime::Builder;
use uuid::Uuid;

struct RemoveDirectoryOnDrop(PathBuf);

impl Drop for RemoveDirectoryOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct PrismaProject {
    directory: PathBuf,
    schema: PathBuf,
    probe: PathBuf,
    _cleanup: RemoveDirectoryOnDrop,
}

fn verify_prisma_toolchain(toolchain: &Path, node: &Path) {
    for (package, expected) in [
        ("prisma", "7.10.0"),
        ("@prisma/client", "7.10.0"),
        ("@prisma/adapter-pg", "7.10.0"),
        ("pg", "8.16.3"),
        ("tsx", "4.20.6"),
    ] {
        assert_eq!(
            read_package_version(toolchain, package),
            expected,
            "unexpected pinned package version for {package}"
        );
    }

    let project_manifest = fs::read_to_string(toolchain.join("package.json"))
        .expect("read isolated toolchain package manifest");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&project_manifest)
            .expect("toolchain package manifest should be valid JSON")["type"],
        "module",
        "Prisma 7 generated client requires the documented ESM project setup"
    );

    let mut node_version_command = Command::new(node);
    node_version_command.arg("--version");
    let node_version = run_external_probe(node_version_command, Duration::from_secs(5))
        .expect("spawn Node version probe");
    assert!(
        node_version.success,
        "Node version probe failed: {}",
        node_version.stderr
    );
    assert_eq!(node_version.stdout.trim(), "v24.21.0");
}

fn create_prisma_project(toolchain: &Path) -> PrismaProject {
    let directory = toolchain.join(format!("probe-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).expect("create temporary Prisma project");
    let cleanup = RemoveDirectoryOnDrop(directory.clone());
    let schema = directory.join("schema.prisma");
    let probe = directory.join("probe.ts");
    fs::write(&schema, PRISMA_SCHEMA).expect("write Prisma schema");
    fs::write(&probe, PRISMA_PROBE).expect("write generated-query probe");

    PrismaProject {
        directory,
        schema,
        probe,
        _cleanup: cleanup,
    }
}

pub(super) fn run_prisma_7_generated_queries_against_cassie() {
    // Arrange
    // Provision an isolated toolchain with Node v24.21.0 and npm:
    //   mkdir -p /tmp/cassie-prisma7-toolchain && cd /tmp/cassie-prisma7-toolchain
    //   npm init -y && npm pkg set type=module
    //   npm install --save-exact --dangerously-allow-all-scripts prisma@7.10.0 @prisma/client@7.10.0 @prisma/adapter-pg@7.10.0 pg@8.16.3 tsx@4.20.6
    // Then run. Fixture DDL failures are reported separately from Prisma SQL:
    //   CASSIE_PRISMA7_TOOLCHAIN=/tmp/cassie-prisma7-toolchain cargo test --locked --test compatibility compatibility_matrix::should_execute_prisma_7_generated_queries_against_cassie -- --ignored --nocapture
    let toolchain = PathBuf::from(
        std::env::var_os("CASSIE_PRISMA7_TOOLCHAIN")
            .expect("set CASSIE_PRISMA7_TOOLCHAIN to the isolated Prisma 7 toolchain directory"),
    );
    let node =
        PathBuf::from(std::env::var_os("CASSIE_PRISMA7_NODE").unwrap_or_else(|| "node".into()));
    verify_prisma_toolchain(&toolchain, &node);
    let project = create_prisma_project(&toolchain);

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    runtime.block_on(run_generated_queries(toolchain, node, project));
}

async fn prepare_prisma_fixtures(
    server: CompatibilityServer,
) -> (CompatibilityServer, Option<String>) {
    let (client, connection_task) = server.connect().await;
    let setup = client
        .batch_execute(
            "CREATE TABLE compat_prisma7_probe (id INT PRIMARY KEY, name TEXT NOT NULL UNIQUE)",
        )
        .await;
    drop(client);
    if let Err(error) = setup {
        server.shutdown(connection_task).await;
        panic!("Cassie rejected the fixture's basic INT/TEXT DDL before any Prisma SQL ran; this is setup failure, not Prisma generated-query evidence: {error}");
    }
    let _ = connection_task.await;

    let (rich_fixture_client, rich_fixture_connection) = server.connect().await;
    let rich_fixture = rich_fixture_client
        .batch_execute(
            "CREATE TABLE compat_prisma7_rich_probe (\
               id INT PRIMARY KEY, \
               tags TEXT[] NOT NULL, \
               payload JSONB NOT NULL, \
               amount NUMERIC(38,10) NOT NULL, \
               occurred_at TIMESTAMPTZ(6) NOT NULL\
             )",
        )
        .await;
    drop(rich_fixture_client);
    let _ = rich_fixture_connection.await;
    if let Err(error) = rich_fixture {
        server.shutdown_without_client().await;
        panic!("Cassie rejected rich-type fixture DDL before Prisma generated SQL ran; this is setup failure, not generated-client evidence: {error}");
    }

    let (enum_fixture_client, enum_fixture_connection) = server.connect().await;
    let enum_fixture = enum_fixture_client
        .batch_execute(
            "CREATE TYPE compat_prisma7_state AS ENUM ('DRAFT', 'READY'); \
             CREATE TABLE compat_prisma7_enum_probe (\
               id INT PRIMARY KEY, \
               state compat_prisma7_state NOT NULL\
             )",
        )
        .await;
    drop(enum_fixture_client);
    let _ = enum_fixture_connection.await;
    (server, enum_fixture.err().map(|error| error.to_string()))
}

async fn run_generated_queries(toolchain: PathBuf, node: PathBuf, project: PrismaProject) {
    let server = CompatibilityServer::start("prisma7_generated_query").await;
    let (server, enum_fixture_error) = prepare_prisma_fixtures(server).await;
    let enum_fixture_enabled = enum_fixture_error.is_none();
    let connection = format!(
        "postgresql://root:postgres@127.0.0.1:{}/postgres",
        server.addr.port()
    );

    let server = generate_prisma_client(server, &node, &toolchain, &project).await;
    let introspection = introspect_prisma_schema(&node, &toolchain, &project, &connection).await;
    let output = execute_generated_query_probe(
        &node,
        &toolchain,
        &project,
        connection,
        enum_fixture_enabled,
    )
    .await;
    let output = match output {
        Ok(result) => result,
        Err(error) => {
            server.shutdown_without_client().await;
            panic!("Prisma generated-query task failed: {error}");
        }
    };

    server.shutdown_without_client().await;

    // Assert
    let output = output.expect("spawn Prisma 7 generated-query probe");
    assert_prisma_probe_completed(&output);
    let events = parse_prisma_query_events(&output);
    assert_prisma_query_event_inventory(&events);
    assert_prisma_create_query(&events);
    assert_prisma_update_and_upsert_queries(&events);
    assert_prisma_pagination_queries(&events);
    assert_prisma_aggregate_queries(&events);
    assert_prisma_transaction_queries(&events);
    assert_prisma_raw_queries(&events);
    assert_prisma_rich_queries(&events);
    assert_prisma_enum_queries(&events, enum_fixture_error.is_none());

    let result = parse_prisma_result(&output.stdout);
    assert_prisma_core_results(&result);
    assert_prisma_typed_results(&result, enum_fixture_error.as_deref());
    assert_prisma_introspection(&introspection);
    assert_eq!(result["queryCount"], events.len());
}

async fn generate_prisma_client(
    server: CompatibilityServer,
    node: &Path,
    toolchain: &Path,
    project: &PrismaProject,
) -> CompatibilityServer {
    let node_for_generate = node.to_path_buf();
    let prisma_cli = toolchain.join("node_modules/prisma/build/index.js");
    let schema_arg = project
        .schema
        .to_str()
        .expect("schema path should be UTF-8")
        .to_string();
    let generation_project = project.directory.clone();
    let generated = tokio::task::spawn_blocking(move || {
        let mut command = Command::new(node_for_generate);
        command
            .arg(prisma_cli)
            .args(["generate", "--schema"])
            .arg(schema_arg)
            .current_dir(generation_project);
        run_external_probe(command, Duration::from_secs(90))
    })
    .await;
    let generated = match generated {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            server.shutdown_without_client().await;
            panic!("spawn Prisma 7 generate: {error}");
        }
        Err(error) => {
            server.shutdown_without_client().await;
            panic!("Prisma generation task failed: {error}");
        }
    };
    if !generated.success {
        server.shutdown_without_client().await;
        panic!(
            "Prisma 7 client generation failed before Cassie query execution; timeout={}, status={:?}\nstdout:\n{}\nstderr:\n{}",
            generated.timed_out, generated.status_code, generated.stdout, generated.stderr
        );
    }
    server
}

async fn introspect_prisma_schema(
    node: &Path,
    toolchain: &Path,
    project: &PrismaProject,
    connection: &str,
) -> ProbeOutput {
    let node_for_introspection = node.to_path_buf();
    let prisma_cli = toolchain.join("node_modules/prisma/build/index.js");
    let schema_arg = project
        .schema
        .to_str()
        .expect("schema path should be UTF-8")
        .to_string();
    let introspection_connection = connection.to_string();
    let introspection_project = project.directory.clone();
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(node_for_introspection);
        command
            .arg(prisma_cli)
            .args(["db", "pull", "--force", "--print", "--schema"])
            .arg(schema_arg)
            .args(["--url"])
            .arg(&introspection_connection)
            .args(["--schemas", "public"])
            .current_dir(introspection_project);
        run_external_probe(command, Duration::from_secs(90))
    })
    .await
    .expect("Prisma introspection blocking task should complete")
    .unwrap_or_else(|error| panic!("spawn Prisma 7 schema introspection: {error}"))
}

async fn execute_generated_query_probe(
    node: &Path,
    toolchain: &Path,
    project: &PrismaProject,
    connection: String,
    enum_fixture_enabled: bool,
) -> Result<Result<ProbeOutput, String>, tokio::task::JoinError> {
    let node_for_probe = node.to_path_buf();
    let tsx_cli = toolchain.join("node_modules/tsx/dist/cli.mjs");
    let probe_path_arg = project
        .probe
        .to_str()
        .expect("probe path should be UTF-8")
        .to_string();
    let probe_project = project.directory.clone();
    tokio::task::spawn_blocking(move || {
        let mut command = Command::new(node_for_probe);
        command
            .arg(tsx_cli)
            .arg(probe_path_arg)
            .env("DATABASE_URL", connection)
            .env(
                "CASSIE_PRISMA7_ENUM_ENABLED",
                if enum_fixture_enabled { "1" } else { "0" },
            )
            .current_dir(probe_project);
        run_external_probe(command, Duration::from_secs(90))
    })
    .await
}
