use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::{CompatibilityServer, run_external_probe};
use tokio::runtime::Builder;
use uuid::Uuid;

const PRISMA_SCHEMA: &str = r#"generator client {
  provider = "prisma-client"
  output   = "./generated"
}

datasource db {
  provider = "postgresql"
}

model PrismaProbe {
  id   Int    @id
  name String @unique

  @@map("compat_prisma7_probe")
}

model PrismaRichProbe {
  id         Int      @id
  tags       String[]
  payload    Json
  amount     Decimal  @db.Decimal(38, 10)
  occurredAt DateTime @db.Timestamptz(6) @map("occurred_at")

  @@map("compat_prisma7_rich_probe")
}

enum PrismaProbeState {
  DRAFT
  READY

  @@map("compat_prisma7_state")
}

model PrismaEnumProbe {
  id    Int              @id
  state PrismaProbeState

  @@map("compat_prisma7_enum_probe")
}
"#;

const PRISMA_PROBE: &str = r#"import { Prisma, PrismaClient } from "./generated/client.ts";
import { PrismaPg } from "@prisma/adapter-pg";
import pg from "pg";

const connectionString = process.env.DATABASE_URL;
if (!connectionString) throw new Error("DATABASE_URL is required");

const preparedQueries = [];
const protocolResults = [];
const protocolErrors = [];
let preparedQueryNumber = 0;
const pool = new pg.Pool({ connectionString });
pool.on("connect", (client) => {
  const originalQuery = client.query.bind(client);
  const captureResult = (config, result) => {
    protocolResults.push({
      name: config.name ?? null,
      query: config.text,
      resultColumns: (result.fields ?? []).map((field) => ({
        name: field.name,
        oid: field.dataTypeID,
        typmod: field.dataTypeModifier,
      })),
    });
  };
  const captureError = (config, error) => {
    protocolErrors.push({
      name: config.name ?? null,
      query: config.text,
      values: config.values ?? [],
      sqlstate: error.code ?? null,
    });
  };
  client.query = (...args) => {
    const config = typeof args[0] === "string"
      ? { text: args[0], values: args[1] }
      : args[0];
    const callbackIndex = args.findIndex((argument) => typeof argument === "function");
    if (callbackIndex !== -1) {
      const callback = args[callbackIndex];
      args[callbackIndex] = (error, result) => {
        if (error) captureError(config, error);
        else captureResult(config, result);
        callback(error, result);
      };
      return originalQuery(...args);
    }
    const execution = originalQuery(...args);
    return execution.then((result) => {
      captureResult(config, result);
      return result;
    }, (error) => {
      captureError(config, error);
      throw error;
    });
  };
});
const adapter = new PrismaPg(pool, {
  disposeExternalPool: true,
  statementNameGenerator(query) {
    const name = `prisma7_probe_${++preparedQueryNumber}`;
    preparedQueries.push({ name, sql: query.sql });
    return name;
  },
  userDefinedTypeParser: async (_oid, value) => value,
});
const prisma = new PrismaClient({ adapter, log: [{ emit: "event", level: "query" }] });
const queries = [];
prisma.$on("query", (event) => {
  queries.push({ query: event.query, params: event.params });
  console.log(`PRISMA7_QUERY ${JSON.stringify({ query: event.query, params: event.params })}`);
});

try {
  const created = await prisma.prismaProbe.create({
    data: {
      id: 41,
      name: "alpha",
    },
  });
  await prisma.prismaProbe.createMany({
    data: [
      { id: 40, name: "seed-40" },
      { id: 43, name: "seed-43" },
    ],
  });
  const page = await prisma.prismaProbe.findMany({
    where: { id: { gte: 40 } },
    orderBy: { id: "asc" },
    skip: 1,
    take: 2,
  });
  const updated = await prisma.prismaProbe.update({
    where: { id: 41 },
    data: { name: "alpha-updated" },
  });
  const upserted = await prisma.prismaProbe.upsert({
    where: { id: 41 },
    update: { name: "alpha-upserted" },
    create: { id: 41, name: "alpha-created" },
  });
  const transaction = await prisma.$transaction([
    prisma.prismaProbe.create({ data: { id: 42, name: "beta" } }),
    prisma.prismaProbe.update({ where: { id: 42 }, data: { name: "beta-committed" } }),
  ]);
  let rollbackError = null;
  try {
    await prisma.$transaction(async (tx) => {
      await tx.prismaProbe.create({ data: { id: 44, name: "rolled-back" } });
      throw new Error("intentional Prisma transaction rollback");
    });
  } catch (error) {
    rollbackError = error.message;
  }
  const rolledBackRow = await prisma.prismaProbe.findUnique({ where: { id: 44 } });
  const aggregate = await prisma.prismaProbe.aggregate({
    _count: { _all: true },
    _max: { id: true },
  });
  const grouped = await prisma.prismaProbe.groupBy({
    by: ["name"],
    _count: { _all: true },
    orderBy: { name: "asc" },
  });
  const deleted = await prisma.prismaProbe.delete({ where: { id: 42 } });
  let duplicateCode = null;
  try {
    await prisma.prismaProbe.create({ data: { id: 99, name: "alpha-upserted" } });
  } catch (error) {
    duplicateCode = error.code;
  }
  const duplicateSqlstate = protocolErrors.find((error) =>
    error.sqlstate === "23505" && error.values.some((value) => String(value) === "99")
  )?.sqlstate ?? null;
  const recoveredCount = await prisma.prismaProbe.count();
  const raw = await prisma.$queryRaw`SELECT ${42}::int4 AS id, ${"wire"}::text AS label`;
  const typedRaw = await prisma.$transaction(async (tx) => {
    const query = `SELECT $1::int4 AS id, $2::text AS label, $3::numeric AS amount`;
    const values = await tx.$queryRaw`SELECT ${42}::int4 AS id, ${"wire"}::text AS label, ${new Prisma.Decimal("123.45")}::numeric AS amount`;
    const prepared = preparedQueries.findLast((entry) => entry.sql === query);
    if (!prepared) throw new Error("typed raw query was not named as a prepared statement");
    const [description] = await tx.$queryRaw`SELECT parameter_types[1]::oid::text || ',' || parameter_types[2]::oid::text || ',' || parameter_types[3]::oid::text AS parameter_oids FROM pg_prepared_statements WHERE name = ${prepared.name}`;
    const resultDescription = protocolResults.findLast((entry) => entry.name === prepared.name);
    if (!resultDescription) throw new Error("typed raw query did not expose its PostgreSQL result fields");
    return {
      values: values.map((row) => ({ id: row.id, label: row.label, amount: row.amount.toString() })),
      parameterOids: description.parameter_oids.split(",").map(Number),
      resultColumns: resultDescription.resultColumns,
    };
  });
  let enumResult = null;
  if (process.env.CASSIE_PRISMA7_ENUM_ENABLED === "1") {
    const enumCreated = await prisma.prismaEnumProbe.create({ data: { id: 45, state: "READY" } });
    const enumSelected = await prisma.prismaEnumProbe.findMany({
      where: { state: "READY" },
      orderBy: { id: "asc" },
    });
    const enumGrouped = await prisma.prismaEnumProbe.groupBy({
      by: ["state"],
      _count: { _all: true },
    });
    enumResult = {
      created: { id: enumCreated.id, state: enumCreated.state },
      selected: enumSelected.map((row) => ({ id: row.id, state: row.state })),
      grouped: enumGrouped.map((row) => ({ state: row.state, count: row._count._all })),
    };
  }
  const richCreated = await prisma.prismaRichProbe.create({
    data: {
      id: 43,
      tags: ["pgwire", "prisma7"],
      payload: { source: "generated-client", attempt: 7, active: true },
      amount: new Prisma.Decimal("12345678901234567890.1234567891"),
      occurredAt: new Date("2024-03-10T06:30:00.123Z"),
    },
  });
  const richRead = await prisma.prismaRichProbe.findUnique({ where: { id: 43 } });
  if (!richRead) throw new Error("Prisma rich-type read returned no row");
  const richStatement = preparedQueries.findLast((entry) =>
    entry.sql.toUpperCase().startsWith("SELECT")
      && entry.sql.toUpperCase().includes('FROM "PUBLIC"."COMPAT_PRISMA7_RICH_PROBE"')
  );
  if (!richStatement) throw new Error("rich delegate read was not prepared by the adapter");
  const richProtocolResult = protocolResults.findLast((entry) => entry.name === richStatement.name);
  if (!richProtocolResult) throw new Error("rich delegate read did not expose PostgreSQL result fields");
  console.log(`PRISMA7_RESULT ${JSON.stringify({
    create: { id: created.id, name: created.name },
    page: page.map((row) => ({ id: row.id, name: row.name })),
    update: { id: updated.id, name: updated.name },
    upsert: { id: upserted.id, name: upserted.name },
    transaction: transaction.map((row) => ({ id: row.id, name: row.name })),
    rollback: { error: rollbackError, rowAbsent: rolledBackRow === null },
    aggregate: { count: aggregate._count._all, maxId: aggregate._max.id },
    grouped: grouped.map((row) => ({ name: row.name, count: row._count._all })),
    delete: { id: deleted.id, name: deleted.name },
    duplicateCode,
    duplicateSqlstate,
    recoveredCount,
    raw,
    typedRaw,
    enumResult,
    richResultColumns: richProtocolResult.resultColumns,
    richCreate: {
      id: richCreated.id,
      tags: richCreated.tags,
      payload: richCreated.payload,
      amount: richCreated.amount.toFixed(10),
      occurredAt: richCreated.occurredAt.toISOString(),
    },
    richRead: {
      id: richRead.id,
      tags: richRead.tags,
      payload: richRead.payload,
      amount: richRead.amount.toFixed(10),
      occurredAt: richRead.occurredAt.toISOString(),
    },
    queryCount: queries.length,
  })}`);
} finally {
  await prisma.$disconnect();
}
"#;

struct RemoveDirectoryOnDrop(PathBuf);

impl Drop for RemoveDirectoryOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_package_version(toolchain: &Path, package: &str) -> String {
    let manifest_path = toolchain
        .join("node_modules")
        .join(package)
        .join("package.json");
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", manifest_path.display()));
    serde_json::from_str::<serde_json::Value>(&manifest)
        .expect("package manifest should be valid JSON")["version"]
        .as_str()
        .expect("package version should be a string")
        .to_string()
}

fn event_query(event: &serde_json::Value) -> &str {
    event["query"]
        .as_str()
        .expect("generated query event should contain SQL text")
}

fn event_parameters(event: &serde_json::Value) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(
        event["params"]
            .as_str()
            .expect("generated query event should contain serialized bind values"),
    )
    .expect("generated query bind values should be JSON")
    .as_array()
    .expect("generated query bind values should be a JSON array")
    .clone()
}

fn assert_placeholders_match_bind_values(event: &serde_json::Value) {
    let query = event_query(event);
    let parameter_count = event_parameters(event).len();
    let query_bytes = query.as_bytes();
    let mut placeholders = Vec::new();
    let mut cursor = 0;
    while cursor + 1 < query_bytes.len() {
        if query_bytes[cursor] == b'$' && query_bytes[cursor + 1].is_ascii_digit() {
            let start = cursor + 1;
            let mut end = start;
            while end < query_bytes.len() && query_bytes[end].is_ascii_digit() {
                end += 1;
            }
            placeholders.push(
                query[start..end]
                    .parse::<usize>()
                    .expect("PostgreSQL placeholder should contain an integer index"),
            );
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    placeholders.sort_unstable();
    placeholders.dedup();

    assert_eq!(
        placeholders,
        (1..=parameter_count).collect::<Vec<_>>(),
        "generated SQL placeholders should match its ordered bind values: SQL={query:?}, params={:?}",
        event["params"]
    );
}

fn has_integer_parameter(parameters: &[serde_json::Value], expected: i64) -> bool {
    parameters.iter().any(|parameter| {
        parameter.as_i64() == Some(expected)
            || parameter
                .as_str()
                .and_then(|value| value.parse::<i64>().ok())
                == Some(expected)
    })
}

fn parameter_contains_text(parameters: &[serde_json::Value], expected: &str) -> bool {
    parameters.iter().any(|parameter| match parameter {
        serde_json::Value::String(value) => value.contains(expected),
        _ => parameter.to_string().contains(expected),
    })
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
    for (package, expected) in [
        ("prisma", "7.10.0"),
        ("@prisma/client", "7.10.0"),
        ("@prisma/adapter-pg", "7.10.0"),
        ("pg", "8.16.3"),
        ("tsx", "4.20.6"),
    ] {
        assert_eq!(
            read_package_version(&toolchain, package),
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

    let mut node_version_command = Command::new(&node);
    node_version_command.arg("--version");
    let node_version = run_external_probe(node_version_command, Duration::from_secs(5))
        .expect("spawn Node version probe");
    assert!(
        node_version.success,
        "Node version probe failed: {}",
        node_version.stderr
    );
    assert_eq!(node_version.stdout.trim(), "v24.21.0");

    let project = toolchain.join(format!("probe-{}", Uuid::new_v4()));
    fs::create_dir_all(&project).expect("create temporary Prisma project");
    let _project_cleanup = RemoveDirectoryOnDrop(project.clone());
    let schema_path = project.join("schema.prisma");
    let probe_path = project.join("probe.ts");
    fs::write(&schema_path, PRISMA_SCHEMA).expect("write Prisma schema");
    fs::write(&probe_path, PRISMA_PROBE).expect("write generated-query probe");

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    runtime.block_on(async {
        let server = CompatibilityServer::start("prisma7_generated_query").await;
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
        let enum_fixture_error = enum_fixture.err().map(|error| error.to_string());
        let enum_fixture_enabled = enum_fixture_error.is_none();

        let connection = format!(
            "postgresql://root:postgres@127.0.0.1:{}/postgres",
            server.addr.port()
        );
        let schema_arg = schema_path
            .to_str()
            .expect("schema path should be UTF-8")
            .to_string();
        let node_for_generate = node.clone();
        let prisma_cli = toolchain.join("node_modules/prisma/build/index.js");
        let generation_project = project.clone();
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

        let node_for_introspection = node.clone();
        let prisma_cli_for_introspection =
            toolchain.join("node_modules/prisma/build/index.js");
        let schema_arg_for_introspection = schema_path
            .to_str()
            .expect("schema path should be UTF-8")
            .to_string();
        let introspection_connection = connection.clone();
        let introspection_project = project.clone();
        let introspection = tokio::task::spawn_blocking(move || {
            let mut command = Command::new(node_for_introspection);
            command
                .arg(prisma_cli_for_introspection)
                .args(["db", "pull", "--force", "--print", "--schema"])
                .arg(schema_arg_for_introspection)
                .args(["--url"])
                .arg(&introspection_connection)
                .args(["--schemas", "public"])
                .current_dir(introspection_project);
            run_external_probe(command, Duration::from_secs(90))
        })
        .await
        .expect("Prisma introspection blocking task should complete")
        .unwrap_or_else(|error| panic!("spawn Prisma 7 schema introspection: {error}"));

        let node_for_probe = node.clone();
        let tsx_cli = toolchain.join("node_modules/tsx/dist/cli.mjs");
        let probe_path_arg = probe_path
            .to_str()
            .expect("probe path should be UTF-8")
            .to_string();
        let probe_project = project.clone();
        let output = tokio::task::spawn_blocking(move || {
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
        assert!(
            !output.timed_out,
            "Prisma 7 generated-query probe timed out\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
        assert!(
            output
                .stdout
                .lines()
                .any(|line| line.starts_with("PRISMA7_QUERY ")),
            "pinned Prisma 7 tooling failed before emitting generated SQL, so this run did not reach a Cassie behavior assertion\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );
        assert!(
            output.success,
            "Prisma 7 generated queries failed against Cassie; this assertion is reached only after the pinned toolchain and generated client are ready\nstdout:\n{}\nstderr:\n{}",
            output.stdout,
            output.stderr
        );

        let events = output
            .stdout
            .lines()
            .filter_map(|line| line.strip_prefix("PRISMA7_QUERY "))
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("query JSON"))
            .collect::<Vec<_>>();
        for event in &events {
            assert_placeholders_match_bind_values(event);
        }
        assert!(
            events.len() >= 12,
            "CRUD, upsert, transaction, aggregate, groupBy, error recovery, raw calls, and rich-type delegates should emit generated SQL: {events:#?}"
        );
        let query_texts = events
            .iter()
            .map(|event| event["query"].as_str().expect("generated SQL"))
            .collect::<Vec<_>>();
        assert!(query_texts.iter().any(|query| {
            query
                .to_ascii_uppercase()
                .contains("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
        }));
        assert!(query_texts.iter().any(|query| {
            query
                .to_ascii_uppercase()
                .contains("UPDATE \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
        }));
        assert!(query_texts
            .iter()
            .any(|query| query.to_ascii_uppercase().contains("ON CONFLICT")));
        assert!(query_texts
            .iter()
            .any(|query| query.to_ascii_uppercase().contains("GROUP BY")));
        assert!(query_texts.iter().any(|query| {
            query
                .to_ascii_uppercase()
                .contains("DELETE FROM \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
        }));
        let insert_event = events
            .iter()
            .find(|event| {
                event_query(event)
                    .starts_with("INSERT INTO \"public\".\"compat_prisma7_probe\"")
            })
            .expect("create should emit an INSERT");
        assert!(
            event_query(insert_event).to_ascii_uppercase().contains("VALUES")
                && event_query(insert_event).contains("$1")
                && event_query(insert_event).contains("$2"),
            "create should bind id and name through the first two PostgreSQL placeholders: {insert_event:#?}"
        );
        assert_eq!(
            event_parameters(insert_event),
            vec![serde_json::json!(41), serde_json::json!("alpha")]
        );

        let update_event = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("UPDATE \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && event_parameters(event).contains(&serde_json::json!("alpha-updated"))
            })
            .expect("delegate update should emit an UPDATE with its requested value");
        assert!(event_query(update_event).contains("$1") && event_query(update_event).contains("$2"));
        let update_parameters = event_parameters(update_event);
        assert_eq!(update_parameters.first(), Some(&serde_json::json!("alpha-updated")));
        assert!(has_integer_parameter(&update_parameters, 41));

        let upsert_event = events
            .iter()
            .find(|event| event_query(event).to_ascii_uppercase().contains("ON CONFLICT"))
            .expect("upsert should emit an ON CONFLICT query");
        let upsert_parameters = event_parameters(upsert_event);
        assert!(upsert_parameters.contains(&serde_json::json!("alpha-upserted")));
        assert!(has_integer_parameter(&upsert_parameters, 41));

        let page_event = events
            .iter()
            .find(|event| {
                event_query(event).contains("LIMIT $2 OFFSET $3")
            })
            .expect("pagination should emit LIMIT and OFFSET SQL");
        let page_query = event_query(page_event);
        assert!(page_query.contains("ORDER BY \"public\".\"compat_prisma7_probe\".\"id\" ASC"));
        assert_eq!(
            event_parameters(page_event),
            vec![
                serde_json::json!(40),
                serde_json::json!("2"),
                serde_json::json!("1"),
            ]
        );
        let create_many_event = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && parameter_contains_text(&event_parameters(event), "seed-40")
                    && parameter_contains_text(&event_parameters(event), "seed-43")
            })
            .expect("createMany should bind both pagination fixture rows");
        assert!(event_query(create_many_event).contains("VALUES"));
        assert!(has_integer_parameter(&event_parameters(create_many_event), 40));
        assert!(has_integer_parameter(&event_parameters(create_many_event), 43));

        let aggregate_event = events
            .iter()
            .find(|event| {
                let query = event_query(event).to_ascii_uppercase();
                query.contains("COUNT(") && query.contains("MAX(")
            })
            .expect("aggregate should emit both COUNT and MAX SQL expressions");
        assert_placeholders_match_bind_values(aggregate_event);
        let grouped_event = events
            .iter()
            .find(|event| {
                let query = event_query(event).to_ascii_uppercase();
                query.contains("GROUP BY") && query.contains("ORDER BY")
            })
            .expect("groupBy should emit grouped and ordered SQL");
        assert_placeholders_match_bind_values(grouped_event);

        let transaction_insert = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && event_parameters(event).contains(&serde_json::json!("beta"))
            })
            .expect("transaction create should emit an INSERT bound to beta");
        assert!(has_integer_parameter(&event_parameters(transaction_insert), 42));
        let transaction_update = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("UPDATE \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && event_parameters(event).contains(&serde_json::json!("beta-committed"))
            })
            .expect("transaction update should bind beta-committed");
        assert!(has_integer_parameter(&event_parameters(transaction_update), 42));

        let rollback_insert = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && parameter_contains_text(&event_parameters(event), "rolled-back")
            })
            .expect("interactive transaction should issue the row that is rolled back");
        assert!(has_integer_parameter(&event_parameters(rollback_insert), 44));

        let delete_event = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("DELETE FROM \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
            })
            .expect("delegate delete should emit a DELETE");
        assert!(has_integer_parameter(&event_parameters(delete_event), 42));

        let duplicate_insert = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_PROBE\"")
                    && event_parameters(event).contains(&serde_json::json!("alpha-upserted"))
                    && has_integer_parameter(&event_parameters(event), 99)
            })
            .expect("duplicate create should emit an INSERT for id 99");
        assert!(event_query(duplicate_insert).contains("$1"));

        let raw_event = events
            .iter()
            .find(|event| event_query(event).contains("SELECT $1::int4 AS id, $2::text AS label"))
            .expect("parameterized raw SQL should be logged");
        assert_eq!(
            event_parameters(raw_event),
            vec![serde_json::json!(42), serde_json::json!("wire")]
        );
        let typed_raw_event = events
            .iter()
            .find(|event| {
                event_query(event)
                    .contains("SELECT $1::int4 AS id, $2::text AS label, $3::numeric AS amount")
            })
            .expect("typed parameter OID query should be logged by Prisma");
        let typed_raw_parameters = event_parameters(typed_raw_event);
        assert!(has_integer_parameter(&typed_raw_parameters, 42));
        assert!(parameter_contains_text(&typed_raw_parameters, "wire"));
        assert!(parameter_contains_text(&typed_raw_parameters, "123.45"));

        let rich_insert = events
            .iter()
            .find(|event| {
                event_query(event)
                    .to_ascii_uppercase()
                    .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_RICH_PROBE\"")
            })
            .expect("rich delegate create should emit an INSERT");
        let rich_insert_sql = event_query(rich_insert).to_ascii_lowercase();
        for column in ["\"tags\"", "\"payload\"", "\"amount\"", "\"occurred_at\""] {
            assert!(
                rich_insert_sql.contains(column),
                "rich create SQL should write {column}: {rich_insert:#?}"
            );
        }
        let rich_insert_parameters = event_parameters(rich_insert);
        assert!(has_integer_parameter(&rich_insert_parameters, 43));
        assert!(parameter_contains_text(&rich_insert_parameters, "pgwire"));
        assert!(parameter_contains_text(&rich_insert_parameters, "prisma7"));
        assert!(parameter_contains_text(&rich_insert_parameters, "generated-client"));
        assert!(parameter_contains_text(
            &rich_insert_parameters,
            "12345678901234567890.1234567891"
        ));
        assert!(parameter_contains_text(
            &rich_insert_parameters,
            "2024-03-10T06:30:00.123Z"
        ));

        let rich_select = events
            .iter()
            .find(|event| {
                let query = event_query(event).to_ascii_uppercase();
                query.starts_with("SELECT")
                    && query.contains("FROM \"PUBLIC\".\"COMPAT_PRISMA7_RICH_PROBE\"")
                    && has_integer_parameter(&event_parameters(event), 43)
            })
            .expect("rich delegate read should emit a parameterized SELECT");
        let rich_select_sql = event_query(rich_select).to_ascii_lowercase();
        for column in ["\"tags\"", "\"payload\"", "\"amount\"", "\"occurred_at\""] {
            assert!(
                rich_select_sql.contains(column),
                "rich read SQL should select {column}: {rich_select:#?}"
            );
        }
        assert!(has_integer_parameter(&event_parameters(rich_select), 43));

        if enum_fixture_error.is_none() {
            let enum_insert = events
                .iter()
                .find(|event| {
                    event_query(event)
                        .to_ascii_uppercase()
                        .starts_with("INSERT INTO \"PUBLIC\".\"COMPAT_PRISMA7_ENUM_PROBE\"")
                })
                .expect("enum delegate create should emit an INSERT");
            assert!(has_integer_parameter(&event_parameters(enum_insert), 45));
            assert!(parameter_contains_text(&event_parameters(enum_insert), "READY"));
            let enum_group = events
                .iter()
                .find(|event| {
                    let query = event_query(event).to_ascii_uppercase();
                    query.contains("COMPAT_PRISMA7_ENUM_PROBE") && query.contains("GROUP BY")
                })
                .expect("enum groupBy should group the generated enum column");
            assert!(event_query(enum_group).to_ascii_uppercase().contains("STATE"));
        }

        let result_line = output
            .stdout
            .lines()
            .find_map(|line| line.strip_prefix("PRISMA7_RESULT "))
            .expect("generated delegate and raw queries should return results");
        let result: serde_json::Value = serde_json::from_str(result_line).expect("result JSON");
        let expected_row = serde_json::json!({
            "id": 41,
            "name": "alpha"
        });
        assert_eq!(result["create"], expected_row);
        assert_eq!(
            result["page"],
            serde_json::json!([
                {"id": 41, "name": "alpha"},
                {"id": 43, "name": "seed-43"}
            ])
        );
        assert_eq!(
            result["update"],
            serde_json::json!({"id": 41, "name": "alpha-updated"})
        );
        assert_eq!(
            result["upsert"],
            serde_json::json!({"id": 41, "name": "alpha-upserted"})
        );
        assert_eq!(
            result["transaction"],
            serde_json::json!([
                {"id": 42, "name": "beta"},
                {"id": 42, "name": "beta-committed"}
            ])
        );
        assert_eq!(
            result["rollback"],
            serde_json::json!({
                "error": "intentional Prisma transaction rollback",
                "rowAbsent": true
            })
        );
        assert_eq!(
            result["aggregate"],
            serde_json::json!({"count": 4, "maxId": 43})
        );
        assert_eq!(
            result["grouped"],
            serde_json::json!([
                {"name": "alpha-upserted", "count": 1},
                {"name": "beta-committed", "count": 1},
                {"name": "seed-40", "count": 1},
                {"name": "seed-43", "count": 1}
            ])
        );
        assert_eq!(
            result["delete"],
            serde_json::json!({"id": 42, "name": "beta-committed"})
        );
        assert_eq!(result["duplicateCode"], "P2002");
        assert_eq!(result["duplicateSqlstate"], "23505");
        assert_eq!(result["recoveredCount"], 3);
        assert_eq!(result["raw"], serde_json::json!([{ "id": 42, "label": "wire" }]));
        assert_eq!(
            result["typedRaw"]["values"],
            serde_json::json!([{"id": 42, "label": "wire", "amount": "123.45"}])
        );
        assert_eq!(result["typedRaw"]["parameterOids"], serde_json::json!([23, 25, 1700]));
        assert_eq!(
            result["typedRaw"]["resultColumns"],
            serde_json::json!([
                {"name": "id", "oid": 23, "typmod": -1},
                {"name": "label", "oid": 25, "typmod": -1},
                {"name": "amount", "oid": 1700, "typmod": -1}
            ])
        );
        assert_eq!(
            result["richResultColumns"],
            serde_json::json!([
                {"name": "id", "oid": 23, "typmod": -1},
                {"name": "tags", "oid": 1009, "typmod": -1},
                {"name": "payload", "oid": 3802, "typmod": -1},
                {"name": "amount", "oid": 1700, "typmod": 2_490_382},
                {"name": "occurred_at", "oid": 1184, "typmod": 6}
            ])
        );
        assert!(enum_fixture_error.is_none(), "Cassie rejected enum fixture DDL before Prisma enum delegates ran; this is an enum setup failure, not generated-query evidence: {enum_fixture_error:?}");
        assert_eq!(
            result["enumResult"],
            serde_json::json!({
                "created": {"id": 45, "state": "READY"},
                "selected": [{"id": 45, "state": "READY"}],
                "grouped": [{"state": "READY", "count": 1}]
            })
        );
        let rich_expected = serde_json::json!({
            "id": 43,
            "tags": ["pgwire", "prisma7"],
            "payload": {"source": "generated-client", "attempt": 7, "active": true},
            "amount": "12345678901234567890.1234567891",
            "occurredAt": "2024-03-10T06:30:00.123Z"
        });
        assert_eq!(result["richCreate"], rich_expected);
        assert_eq!(result["richRead"], rich_expected);
        assert!(!introspection.timed_out, "pinned Prisma db pull timed out\nstdout:\n{}\nstderr:\n{}", introspection.stdout, introspection.stderr);
        assert!(introspection.success, "pinned Prisma schema discovery failed against Cassie\nstdout:\n{}\nstderr:\n{}", introspection.stdout, introspection.stderr);
        for expected in [
            "compat_prisma7_probe",
            "compat_prisma7_rich_probe",
            "tags",
            "payload",
            "amount",
            "occurred_at",
            "compat_prisma7_state",
            "compat_prisma7_enum_probe",
        ] {
            assert!(introspection.stdout.contains(expected), "Prisma 7 schema discovery should include {expected:?}: {}", introspection.stdout);
        }
        assert_eq!(result["queryCount"], events.len());
    });
}
