//! Pgwire visibility of the selected staging-table rebuild swap.

use super::*;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

async fn table_row(client: &tokio_postgres::Client, sql: &str) -> (i64, String) {
    let row = client
        .query_one(sql, &[])
        .await
        .expect("read rebuild table");
    (row.get(0), row.get(1))
}

async fn table_rows(client: &tokio_postgres::Client, sql: &str) -> Vec<(i64, String)> {
    client
        .query(sql, &[])
        .await
        .expect("read rebuild table rows")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

async fn simple_query(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) -> Vec<(u8, Vec<u8>)> {
    support_pgwire::write_frames(writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    support_pgwire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

async fn copy_from_stdin(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
    data: &[u8],
) -> Vec<(u8, Vec<u8>)> {
    support_pgwire::write_frames(writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    let response = support_pgwire::read_wire_frame(reader).await;
    if response.0 != b'G' {
        let mut frames =
            support_pgwire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
        frames.insert(0, response);
        return frames;
    }

    support_pgwire::write_frames(
        writer,
        vec![
            support_pgwire::copy_data_frame(data),
            support_pgwire::copy_done_frame(),
        ],
    )
    .await;
    let mut frames =
        support_pgwire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
    frames.insert(0, response);
    frames
}

async fn wire_rows(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) -> Vec<Vec<Option<String>>> {
    let frames = simple_query(reader, writer, sql).await;
    assert_eq!(
        support_pgwire::error_code(&frames),
        None,
        "query failed: {sql}"
    );
    support_pgwire::data_rows(&frames)
}

async fn table_catalog(client: &tokio_postgres::Client) -> Vec<(String, i64)> {
    client
        .query(
            "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
             WHERE relname IN ('wire_rebuild_live', 'wire_rebuild_old', 'wire_rebuild_stage') \
             ORDER BY relname",
            &[],
        )
        .await
        .expect("read rebuild table identities")
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect()
}

#[derive(Clone, Copy)]
struct RelationOids {
    live: i64,
    stage: i64,
}

fn initial_wire_catalog(oids: RelationOids) -> Vec<Vec<Option<String>>> {
    vec![
        vec![
            Some("wire_rebuild_live".to_owned()),
            Some(oids.live.to_string()),
        ],
        vec![
            Some("wire_rebuild_stage".to_owned()),
            Some(oids.stage.to_string()),
        ],
    ]
}

fn swapped_catalog(oids: RelationOids) -> Vec<(String, i64)> {
    vec![
        ("wire_rebuild_live".to_owned(), oids.stage),
        ("wire_rebuild_old".to_owned(), oids.live),
    ]
}

fn swapped_wire_catalog(oids: RelationOids) -> Vec<Vec<Option<String>>> {
    vec![
        vec![
            Some("wire_rebuild_live".to_owned()),
            Some(oids.stage.to_string()),
        ],
        vec![
            Some("wire_rebuild_old".to_owned()),
            Some(oids.live.to_string()),
        ],
    ]
}

async fn stage_copy_and_assert_swap(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    observer: &tokio_postgres::Client,
    original_catalog: &[(String, i64)],
    oids: RelationOids,
) {
    let begin = simple_query(reader, writer, "BEGIN").await;
    assert_eq!(support_pgwire::error_code(&begin), None);
    let copy = copy_from_stdin(
        reader,
        writer,
        "COPY wire_rebuild_stage FROM STDIN WITH (FORMAT CSV)",
        b"2,new\n",
    )
    .await;
    assert_eq!(copy.first().map(|(tag, _)| *tag), Some(b'G'));
    assert_eq!(support_pgwire::error_code(&copy), None);
    assert_eq!(
        copy.last().map(|(_, payload)| payload.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT id, label FROM wire_rebuild_stage ORDER BY id"
        )
        .await,
        vec![vec![Some("2".to_owned()), Some("new".to_owned())]]
    );

    for sql in [
        "ALTER TABLE wire_rebuild_live RENAME TO wire_rebuild_old",
        "ALTER TABLE wire_rebuild_stage RENAME TO wire_rebuild_live",
    ] {
        let frames = simple_query(reader, writer, sql).await;
        assert_eq!(
            support_pgwire::error_code(&frames),
            None,
            "rename failed: {sql}"
        );
    }
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT id, label FROM wire_rebuild_live ORDER BY id"
        )
        .await,
        vec![vec![Some("2".to_owned()), Some("new".to_owned())]]
    );
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT id, label FROM wire_rebuild_old ORDER BY id"
        )
        .await,
        vec![vec![Some("1".to_owned()), Some("old".to_owned())]]
    );
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
             WHERE relname IN ('wire_rebuild_live', 'wire_rebuild_old', 'wire_rebuild_stage') \
             ORDER BY relname",
        )
        .await,
        swapped_wire_catalog(oids)
    );
    assert_eq!(
        table_row(observer, "SELECT id, label FROM wire_rebuild_live").await,
        (1, "old".to_owned())
    );
    assert_eq!(
        table_rows(observer, "SELECT id, label FROM wire_rebuild_stage").await,
        Vec::<(i64, String)>::new()
    );
    assert_eq!(table_catalog(observer).await, original_catalog);
}

async fn rollback_and_assert_original_state(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    observer: &tokio_postgres::Client,
    original_catalog: &[(String, i64)],
    oids: RelationOids,
) {
    let rollback = simple_query(reader, writer, "ROLLBACK").await;
    assert_eq!(support_pgwire::error_code(&rollback), None);
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT id, label FROM wire_rebuild_live ORDER BY id"
        )
        .await,
        vec![vec![Some("1".to_owned()), Some("old".to_owned())]]
    );
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT id, label FROM wire_rebuild_stage ORDER BY id",
        )
        .await,
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        wire_rows(
            reader,
            writer,
            "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
             WHERE relname IN ('wire_rebuild_live', 'wire_rebuild_old', 'wire_rebuild_stage') \
             ORDER BY relname",
        )
        .await,
        initial_wire_catalog(oids)
    );
    assert_eq!(table_catalog(observer).await, original_catalog);
}

async fn commit_and_assert_swapped_state(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    observer: &tokio_postgres::Client,
    oids: RelationOids,
) {
    let commit = simple_query(reader, writer, "COMMIT").await;
    assert_eq!(support_pgwire::error_code(&commit), None);
    assert_eq!(
        table_row(observer, "SELECT id, label FROM wire_rebuild_live").await,
        (2, "new".to_owned())
    );
    assert_eq!(
        table_row(observer, "SELECT id, label FROM wire_rebuild_old").await,
        (1, "old".to_owned())
    );
    assert_eq!(table_catalog(observer).await, swapped_catalog(oids));
}

#[test]
fn should_publish_rebuild_swap_to_other_pgwire_sessions_only_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_826_rebuild_swap_wire").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE wire_rebuild_live (id BIGINT, label TEXT)",
                "CREATE TABLE wire_rebuild_stage (id BIGINT, label TEXT)",
                "INSERT INTO wire_rebuild_live VALUES (1, 'old')",
                "INSERT INTO wire_rebuild_stage VALUES (2, 'new')",
            ],
        )
        .await
        .expect("create and load separate live and staging tables");

        // Act
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "ALTER TABLE wire_rebuild_live RENAME TO wire_rebuild_old",
                "ALTER TABLE wire_rebuild_stage RENAME TO wire_rebuild_live",
            ],
        )
        .await
        .expect("stage both renames as separate protocol statements");
        let owner_live = table_row(&owner, "SELECT id, label FROM wire_rebuild_live").await;
        let owner_old = table_row(&owner, "SELECT id, label FROM wire_rebuild_old").await;
        let observer_live = table_row(&observer, "SELECT id, label FROM wire_rebuild_live").await;
        let observer_stage = table_row(&observer, "SELECT id, label FROM wire_rebuild_stage").await;
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back swap");
        let rollback_live = table_row(&owner, "SELECT id, label FROM wire_rebuild_live").await;
        let rollback_stage = table_row(&owner, "SELECT id, label FROM wire_rebuild_stage").await;
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "ALTER TABLE wire_rebuild_live RENAME TO wire_rebuild_old",
                "ALTER TABLE wire_rebuild_stage RENAME TO wire_rebuild_live",
                "COMMIT",
            ],
        )
        .await
        .expect("retry and commit both renames");
        let committed_live = table_row(&observer, "SELECT id, label FROM wire_rebuild_live").await;
        let committed_old = table_row(&observer, "SELECT id, label FROM wire_rebuild_old").await;
        let stage_count: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'wire_rebuild_stage'",
                &[],
            )
            .await
            .expect("check the former staging name after commit")
            .get(0);

        // Assert
        let old = (1, "old".to_owned());
        let new = (2, "new".to_owned());
        assert_eq!(owner_live, new);
        assert_eq!(owner_old, old);
        assert_eq!(observer_live, old);
        assert_eq!(observer_stage, new);
        assert_eq!(rollback_live, old);
        assert_eq!(rollback_stage, new);
        assert_eq!(committed_live, new);
        assert_eq!(committed_old, old);
        assert_eq!(stage_count, 0);

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_publish_copy_loaded_staging_swap_atomically() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_826_copy_rebuild_swap").await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect raw pgwire owner");
        let (mut owner_reader, mut owner_writer) = socket.into_split();
        support_pgwire::complete_startup(&mut owner_reader, &mut owner_writer).await;
        let (observer, observer_connection) = server.connect().await;

        for sql in [
            "CREATE TABLE wire_rebuild_live (id BIGINT, label TEXT)",
            "CREATE TABLE wire_rebuild_stage (id BIGINT, label TEXT)",
            "INSERT INTO wire_rebuild_live VALUES (1, 'old')",
        ] {
            let frames = simple_query(&mut owner_reader, &mut owner_writer, sql).await;
            assert_eq!(
                support_pgwire::error_code(&frames),
                None,
                "setup failed: {sql}"
            );
        }
        let original_catalog = table_catalog(&observer).await;
        assert_eq!(original_catalog.len(), 2);
        let oids = RelationOids {
            live: original_catalog
                .iter()
                .find(|(name, _)| name == "wire_rebuild_live")
                .expect("live relation identity")
                .1,
            stage: original_catalog
                .iter()
                .find(|(name, _)| name == "wire_rebuild_stage")
                .expect("staging relation identity")
                .1,
        };
        assert_ne!(oids.live, oids.stage);

        // Act
        // Assert each owner and observer view at the transaction boundaries.
        stage_copy_and_assert_swap(
            &mut owner_reader,
            &mut owner_writer,
            &observer,
            &original_catalog,
            oids,
        )
        .await;
        rollback_and_assert_original_state(
            &mut owner_reader,
            &mut owner_writer,
            &observer,
            &original_catalog,
            oids,
        )
        .await;
        stage_copy_and_assert_swap(
            &mut owner_reader,
            &mut owner_writer,
            &observer,
            &original_catalog,
            oids,
        )
        .await;
        commit_and_assert_swapped_state(&mut owner_reader, &mut owner_writer, &observer, oids)
            .await;

        // Assert
        assert_eq!(table_catalog(&observer).await, swapped_catalog(oids));

        drop((owner_reader, owner_writer, observer));
        server.shutdown(observer_connection).await;
    });
}
