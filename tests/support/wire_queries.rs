//! Bounded pgwire query cycles and success assertions shared by SQL and codec suites.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::types::Value;

use super::VECTOR_TYPE_OID;

pub type Frames = Vec<(u8, Vec<u8>)>;

pub type Cycle = Vec<Vec<u8>>;

/// # Panics
///
/// Panics if the supplied test data is invalid or a required fixture operation fails.
#[must_use]
pub fn run_cycles(
    label: &str,
    setup: &[(&str, Vec<Value>)],
    mut cycles: Vec<Cycle>,
) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        if config.password.is_empty() {
            crate::support_sql::test_root_password().clone_into(&mut config.password);
        }
        let cassie = Cassie::new_in_memory_with_config(config)
            .unwrap_or_else(|error| panic!("create ephemeral fixture {label}: {error}"));
        cassie.startup().expect("startup before table creation");
        let session = cassie.create_session("tester", None);
        if cycles.iter().flatten().any(|frame| parse_frame_has_vector_placeholder(frame)) {
            let database = session.current_database().unwrap_or("postgres");
            if cassie.catalog.get_extension(database, "vector").is_none() {
                let root = cassie.create_session("root", None);
                cassie
                    .execute_sql(
                        &root,
                        "CREATE EXTENSION vector VERSION '0.8.7'",
                        Vec::new(),
                    )
                    .expect("install vector for dynamic-OID wire fixtures");
            }
            let database = session.current_database().unwrap_or("postgres");
            let vector_oid = cassie
                .catalog
                .get_extension(database, "vector")
                .and_then(|extension| extension.vector_type_object_id)
                .and_then(cassie::catalog::object_identity::normal_oid)
                .and_then(|oid| i32::try_from(oid).ok())
                .expect("installed vector type has a normal OID");
            for frame in cycles.iter_mut().flatten() {
                rewrite_parse_frame_vector_placeholder(frame, vector_oid);
            }
        }
        for (sql, params) in setup {
            cassie
                .execute_sql(&session, sql, params.clone())
                .expect("seed finite metadata fixture");
        }
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            wire::write_frames(&mut writer, cycle).await;
            batches.push(
                wire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });
    batches
}

fn parse_frame_has_vector_placeholder(frame: &[u8]) -> bool {
    parse_parameter_oids(frame).is_some_and(|(start, count)| {
        (0..count).any(|index| {
            let offset = start.saturating_add(index.saturating_mul(4));
            frame
                .get(offset..offset.saturating_add(4))
                .is_some_and(|bytes| {
                    i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                        == VECTOR_TYPE_OID
                })
        })
    })
}

fn rewrite_parse_frame_vector_placeholder(frame: &mut [u8], vector_oid: i32) {
    let Some((start, count)) = parse_parameter_oids(frame) else {
        return;
    };
    for index in 0..count {
        let offset = start.saturating_add(index.saturating_mul(4));
        let Some(bytes) = frame.get(offset..offset.saturating_add(4)) else {
            return;
        };
        if i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
            == VECTOR_TYPE_OID
        {
            frame[offset..offset + 4].copy_from_slice(&vector_oid.to_be_bytes());
        }
    }
}

fn parse_parameter_oids(frame: &[u8]) -> Option<(usize, usize)> {
    if frame.first() != Some(&b'P') || frame.len() < 5 {
        return None;
    }
    let mut cursor = 5;
    for _ in 0..2 {
        let end = frame.get(cursor..)?.iter().position(|byte| *byte == 0)?;
        cursor = cursor.checked_add(end)?.checked_add(1)?;
    }
    let count_bytes = frame.get(cursor..cursor.checked_add(2)?)?;
    let count = usize::from(u16::from_be_bytes([count_bytes[0], count_bytes[1]]));
    let start = cursor.checked_add(2)?;
    let end = start.checked_add(count.checked_mul(4)?)?;
    (end <= frame.len()).then_some((start, count))
}

/// # Panics
///
/// Panics when the observed wire frames do not match the expected test contract.
pub fn assert_success(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire errors: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}
