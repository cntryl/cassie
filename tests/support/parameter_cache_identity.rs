//! Fresh pgwire sessions sharing one engine expose parameter cache identity.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::{self as metadata, Frames};
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use cassie::runtime::ExecutionMode;
use cassie::types::Value;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Case {
    pub sql: &'static str,
    pub oids: &'static [i32],
    pub inputs: &'static [&'static str],
    pub columns: &'static [(i32, i16)],
    pub outputs: &'static [&'static [u8]],
}

pub struct Evidence {
    pub statement: Frames,
    pub execution: Frames,
    pub metrics: serde_json::Value,
}

pub fn run(cases: &[Case], result_cache: bool) -> Vec<Evidence> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("parameter cache runtime");
    runtime.block_on(async {
        let config = CassieRuntimeConfig {
            limits: CassieRuntimeLimits {
                execution_result_cache_enabled: if result_cache {
                    ExecutionResultCacheEnabled::enabled()
                } else {
                    ExecutionResultCacheEnabled::disabled()
                },
                ..CassieRuntimeLimits::default()
            },
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_in_memory_with_config(config).expect("ephemeral cache fixture");
        cassie.startup().expect("cache fixture startup");
        let server = wire::spawn_server(cassie.clone()).await;
        let mut evidence = Vec::new();
        for case in cases {
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("fresh cache session");
            let (mut reader, mut writer) = tokio::io::split(socket);
            wire::complete_startup(&mut reader, &mut writer).await;
            wire::write_frames(
                &mut writer,
                vec![
                    wire::parse_frame_with_types("statement", case.sql, case.oids),
                    wire::describe_statement_frame("statement"),
                    wire::sync_frame(),
                ],
            )
            .await;
            let statement =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
            let inputs = case
                .inputs
                .iter()
                .map(|input| Some(input.as_bytes()))
                .collect::<Vec<_>>();
            wire::write_frames(
                &mut writer,
                vec![
                    wire::bind_frame_with_formats("portal", "statement", &[0], &inputs, &[1]),
                    wire::describe_portal_frame("portal"),
                    wire::execute_frame("portal"),
                    wire::sync_frame(),
                ],
            )
            .await;
            let execution =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
            evidence.push(Evidence {
                statement,
                execution,
                metrics: cassie.metrics(),
            });
            drop((reader, writer));
        }
        server.stop().await;
        evidence
    })
}

pub fn assert_case(evidence: &Evidence, case: Case) {
    metadata::assert_success(&evidence.statement);
    metadata::assert_success(&evidence.execution);
    let parameters = evidence
        .statement
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), case.oids);
    for (frames, format) in [(&evidence.statement, 0), (&evidence.execution, 1)] {
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("RowDescription");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(columns.len(), case.columns.len());
        for (column, (oid, size)) in columns.iter().zip(case.columns) {
            assert_eq!(
                (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code
                ),
                (*oid, *size, -1, format),
                "input OIDs {:?} for {}",
                case.oids,
                case.sql,
            );
        }
    }
    let mut row = i16::try_from(case.outputs.len())
        .expect("column count")
        .to_be_bytes()
        .to_vec();
    for output in case.outputs {
        row.extend_from_slice(
            &i32::try_from(output.len())
                .expect("field length")
                .to_be_bytes(),
        );
        row.extend_from_slice(output);
    }
    assert_eq!(
        metadata::data_row_payloads(&evidence.execution),
        [row],
        "input OIDs {:?} for {}",
        case.oids,
        case.sql,
    );
}

pub fn counter(evidence: &Evidence, cache: &str, name: &str) -> u64 {
    evidence.metrics[cache][name]
        .as_u64()
        .expect("cache counter")
}

pub fn run_orders(cases: &[Case], result_cache: bool) -> Vec<Vec<Evidence>> {
    let reversed = cases.iter().rev().copied().collect::<Vec<_>>();
    [cases, reversed.as_slice()]
        .map(|order| run(order, result_cache))
        .into()
}

pub fn assert_orders(cases: &[Case], evidence: &[Vec<Evidence>]) {
    let reversed = cases.iter().rev().copied().collect::<Vec<_>>();
    for (order, evidence) in [cases, reversed.as_slice()].into_iter().zip(evidence) {
        for (evidence, case) in evidence.iter().zip(order) {
            assert_case(evidence, *case);
        }
    }
}

pub fn diagnostic_hit(data_type: &str, input: &str) -> bool {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("diagnostic engine");
    cassie.startup().expect("diagnostic startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE diagnostic_inputs (value {data_type})"),
            vec![],
        )
        .expect("diagnostic relation");
    let sql = "SELECT value FROM diagnostic_inputs WHERE value = $1";
    let parameters = vec![Value::String(input.to_owned())];
    cassie
        .execute_sql(&session, sql, parameters.clone())
        .expect("populate diagnostic plan cache");
    cassie.plan_cache_hit_for_diagnostics(
        &cassie::sql::parse_statement(sql).expect("diagnostic SQL"),
        &parameters,
        ExecutionMode::SimpleQuery,
        session.database.clone(),
        &session.search_path(),
    )
}
