//! PostgreSQL 18.6 mutation and data-modifying CTE gaps for #823, #825 and #834.

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use super::support_sql_fixture::sql_fixture;

#[path = "postgresql18_mutation_inventory_wave4/data_modifying_cte_wire_atomicity.rs"]
mod data_modifying_cte_wire_atomicity;
#[path = "postgresql18_mutation_inventory_wave4/merge_cardinality_atomicity.rs"]
mod merge_cardinality_atomicity;
#[path = "postgresql18_mutation_inventory_wave4/returning_error_order.rs"]
mod returning_error_order;
#[path = "postgresql18_mutation_inventory_wave4/truncate_restart_identity_commit.rs"]
mod truncate_restart_identity_commit;
#[path = "postgresql18_mutation_inventory_wave4/update_returning_descriptor.rs"]
mod update_returning_descriptor;

struct PgwireFixture {
    client: Option<Client>,
    connection: Option<JoinHandle<()>>,
    server: Option<JoinHandle<Result<(), CassieError>>>,
    data_dir: String,
}

impl PgwireFixture {
    async fn start(label: &str) -> Self {
        super::support_sql::use_local_storage();
        let data_dir = super::support_sql::data_dir(label);
        let cassie = Cassie::new_with_data_dir(&data_dir).expect("create Cassie");
        cassie.startup().expect("start Cassie");

        let config = CassieRuntimeConfig::from_env().expect("runtime config");
        let password = config.password.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address: SocketAddr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut client_config = tokio_postgres::Config::new();
        client_config
            .host("127.0.0.1")
            .port(address.port())
            .user("root")
            .password(&password)
            .dbname("postgres");
        let (client, connection) = client_config
            .connect(NoTls)
            .await
            .expect("connect pgwire client");
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });

        Self {
            client: Some(client),
            connection: Some(connection),
            server: Some(server),
            data_dir,
        }
    }

    fn client(&self) -> &Client {
        self.client.as_ref().expect("live pgwire client")
    }

    async fn shutdown(mut self) {
        drop(self.client.take());
        if let Some(connection) = self.connection.take() {
            connection.abort();
            let _ = connection.await;
        }
        if let Some(server) = self.server.take() {
            server.abort();
            let _ = server.await;
        }
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

impl Drop for PgwireFixture {
    fn drop(&mut self) {
        drop(self.client.take());
        if let Some(connection) = self.connection.take() {
            connection.abort();
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn db_error(error: &tokio_postgres::Error) -> &tokio_postgres::error::DbError {
    error.as_db_error().expect("PostgreSQL database error")
}

#[test]
fn should_reject_truncate_restrict_before_changing_any_listed_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-823-truncate-restrict-atomicity").await;
        let setup_statements = [
            "CREATE TABLE wave4_truncate_parent (id INT PRIMARY KEY)",
            "CREATE TABLE wave4_truncate_child (id INT PRIMARY KEY, parent_id INT \
               REFERENCES wave4_truncate_parent(id))",
            "CREATE TABLE wave4_truncate_unrelated (id INT PRIMARY KEY)",
            "INSERT INTO wave4_truncate_parent VALUES (1)",
            "INSERT INTO wave4_truncate_child VALUES (10, 1)",
            "INSERT INTO wave4_truncate_unrelated VALUES (99)",
        ];
        for sql in setup_statements {
            if let Err(error) = fixture.client().batch_execute(sql).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {sql}: {error:?}");
            }
        }

        // Act
        let rejected = fixture
            .client()
            .batch_execute(
                "TRUNCATE TABLE wave4_truncate_unrelated, wave4_truncate_parent RESTRICT",
            )
            .await;
        let counts = fixture
            .client()
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM wave4_truncate_parent), \
                   (SELECT count(*) FROM wave4_truncate_child), \
                   (SELECT count(*) FROM wave4_truncate_unrelated)",
                &[],
            )
            .await;
        fixture.shutdown().await;

        // Assert
        let error = rejected.expect_err("RESTRICT must reject the referenced parent");
        let database_error = db_error(&error);
        assert_eq!(database_error.code().code(), "0A000");
        assert_eq!(
            database_error.message(),
            "cannot truncate a table referenced in a foreign key constraint"
        );
        assert!(database_error
            .detail()
            .expect("referencing table detail")
            .contains("wave4_truncate_child"));
        let counts = counts.expect("read rows after rejected TRUNCATE");
        assert_eq!(
            (
                counts.get::<_, i64>(0),
                counts.get::<_, i64>(1),
                counts.get::<_, i64>(2),
            ),
            (1, 1, 1),
            "RESTRICT rejection must not truncate another table from the same command"
        );
    });
}

#[test]
fn should_expose_updated_cte_rows_beside_the_original_statement_snapshot() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-update-cte-snapshot-wave4",
        &[
            "CREATE TABLE issue_834_update_snapshot_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_snapshot_wave4 VALUES (1, 'old'), (2, 'keep')",
        ],
    );

    // Act
    let observed = fixture.execute(
        "WITH changed AS ( \
             UPDATE issue_834_update_snapshot_wave4 SET payload = 'new' WHERE id = 1 \
             RETURNING id, payload \
         ), snapshot AS ( \
             SELECT id, payload FROM issue_834_update_snapshot_wave4 \
         ) \
         SELECT 'changed' AS source, id, payload FROM changed \
         UNION ALL \
         SELECT 'snapshot', id, payload FROM snapshot \
         ORDER BY source, id",
    );
    let stored =
        fixture.execute("SELECT id, payload FROM issue_834_update_snapshot_wave4 ORDER BY id");

    // Assert
    assert_eq!(
        observed
            .expect("read UPDATE RETURNING and sibling statement snapshot")
            .rows(),
        &[
            vec![
                Value::String("changed".to_owned()),
                Value::Int64(1),
                Value::String("new".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(1),
                Value::String("old".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(2),
                Value::String("keep".to_owned()),
            ],
        ]
    );
    assert_eq!(
        stored.expect("read table after UPDATE CTE").rows(),
        &[
            vec![Value::Int64(1), Value::String("new".to_owned())],
            vec![Value::Int64(2), Value::String("keep".to_owned())],
        ]
    );
}

#[test]
fn should_rollback_update_cte_when_its_outer_insert_hits_a_unique_conflict() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-update-cte-atomicity-wave4",
        &[
            "CREATE TABLE issue_834_update_cte_source_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "CREATE TABLE issue_834_update_cte_target_wave4 (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_update_cte_source_wave4 VALUES (1, 'source-one'), (2, 'source-two')",
            "INSERT INTO issue_834_update_cte_target_wave4 VALUES (2, 'existing-target')",
        ],
    );

    // Act
    let failed = fixture.execute(
        "WITH changed AS ( \
             UPDATE issue_834_update_cte_source_wave4 SET payload = 'updated' \
             RETURNING id, payload \
         ) \
         INSERT INTO issue_834_update_cte_target_wave4 (id, payload) \
         SELECT id, payload FROM changed ORDER BY id",
    );
    let source =
        fixture.execute("SELECT id, payload FROM issue_834_update_cte_source_wave4 ORDER BY id");
    let target =
        fixture.execute("SELECT id, payload FROM issue_834_update_cte_target_wave4 ORDER BY id");

    // Assert
    assert!(
        matches!(failed, Err(CassieError::UniqueViolation { .. })),
        "outer INSERT must report the 23505 duplicate key, got {failed:?}"
    );
    assert_eq!(
        source
            .expect("read source after rejected outer INSERT")
            .rows(),
        &[
            vec![Value::Int64(1), Value::String("source-one".to_owned())],
            vec![Value::Int64(2), Value::String("source-two".to_owned())],
        ],
        "the UPDATE CTE must roll back with the failing outer INSERT"
    );
    assert_eq!(
        target
            .expect("read target after rejected outer INSERT")
            .rows(),
        &[vec![
            Value::Int64(2),
            Value::String("existing-target".to_owned()),
        ]]
    );
}
