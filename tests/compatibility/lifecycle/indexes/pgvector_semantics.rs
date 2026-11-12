use super::*;
use bytes::{BufMut, BytesMut};
use tokio::task::JoinHandle;
use tokio_postgres::types::{to_sql_checked, IsNull, ToSql, Type};

const FIXTURE_ROWS: &str = "INSERT INTO pgvector_semantics VALUES \
     (1, '[1,0,0]'::vector), (2, '[0,2,0]'::vector), (3, '[1,1,0]'::vector), \
     (4, '[-1,0,1]'::vector), (5, '[2,2,2]'::vector)";

/// Binary `vector` parameter: int16 dimensions, int16 reserved zero, big-endian f32s.
#[derive(Debug)]
struct VectorParam(Vec<f32>);

impl ToSql for VectorParam {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        out.put_i16(i16::try_from(self.0.len())?);
        out.put_i16(0);
        for component in &self.0 {
            out.put_f32(*component);
        }
        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        ty.name() == "vector"
    }

    to_sql_checked!();
}

async fn start_fixture(
    name: &str,
) -> (CompatibilityServer, tokio_postgres::Client, JoinHandle<()>) {
    let server = CompatibilityServer::start(name).await;
    let (client, connection) = server.connect().await;
    execute_statements(
        &client,
        &[
            "CREATE EXTENSION vector WITH SCHEMA public VERSION '0.8.7'",
            "CREATE TABLE pgvector_semantics (id INT, embedding VECTOR(3))",
            FIXTURE_ROWS,
        ],
    )
    .await
    .expect("create the deterministic pgvector fixture");
    (server, client, connection)
}

async fn top_ids(client: &tokio_postgres::Client, operator: &str) -> Vec<i32> {
    let sql = format!(
        "SELECT id, embedding {operator} '[3,1,0]'::vector AS distance FROM pgvector_semantics \
         ORDER BY embedding {operator} '[3,1,0]'::vector LIMIT 3"
    );
    row_ids(&client.query(&sql, &[]).await.expect("query top-k rows"))
}

/// Returns the unindexed top-k followed by the top-k with an `HNSW` and then an
/// `IVFFlat` index of `opclass` present. The SQL operator ANN execution path serves
/// L2 only, so inner-product and cosine orderings must equal the exact scan.
async fn top_k_without_and_with_indexes(
    name: &str,
    operator: &str,
    opclass: &str,
) -> [Vec<i32>; 3] {
    let (server, client, connection) = start_fixture(name).await;
    let exact = top_ids(&client, operator).await;
    let mut results = [exact, Vec::new(), Vec::new()];
    let methods = [
        ("hnsw", "WITH (m = 4, ef_construction = 16)"),
        ("ivfflat", "WITH (lists = 1)"),
    ];
    for (slot, (method, options)) in methods.into_iter().enumerate() {
        client
            .batch_execute(&format!(
                "CREATE INDEX pgvector_semantics_idx ON pgvector_semantics \
                 USING {method} (embedding {opclass}) {options}"
            ))
            .await
            .expect("create the ANN index");
        results[slot + 1] = top_ids(&client, operator).await;
        client
            .batch_execute("DROP INDEX pgvector_semantics_idx")
            .await
            .expect("drop the ANN index");
    }
    drop(client);
    server.shutdown(connection).await;
    results
}

#[test]
fn should_match_exact_inner_product_top_k_with_pgvector_indexes_present() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let [exact, hnsw, ivfflat] = runtime.block_on(top_k_without_and_with_indexes(
        "pgvector_ip_top_k",
        "<#>",
        "vector_ip_ops",
    ));

    // Assert
    assert_eq!(exact, vec![5, 3, 1]);
    assert_eq!(hnsw, exact, "HNSW vector_ip_ops equals the exact top-k");
    assert_eq!(
        ivfflat, exact,
        "IVFFlat vector_ip_ops equals the exact top-k"
    );
}

#[test]
fn should_match_exact_cosine_top_k_with_pgvector_indexes_present() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let [exact, hnsw, ivfflat] = runtime.block_on(top_k_without_and_with_indexes(
        "pgvector_cosine_top_k",
        "<=>",
        "vector_cosine_ops",
    ));

    // Assert
    assert_eq!(exact, vec![1, 3, 5]);
    assert_eq!(hnsw, exact, "HNSW vector_cosine_ops equals the exact top-k");
    assert_eq!(
        ivfflat, exact,
        "IVFFlat vector_cosine_ops equals the exact top-k"
    );
}

#[test]
fn should_return_pgvector_operator_values() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_operator_values").await;

        // Act
        let row = client
            .query_one(
                "SELECT embedding <#> '[3,1,0]'::vector, embedding <=> '[3,1,0]'::vector, \
                        embedding <-> '[3,1,0]'::vector \
                 FROM pgvector_semantics WHERE id = 5",
                &[],
            )
            .await
            .expect("evaluate the three pgvector operators");

        // Assert
        let expected_cosine = 1.0 - 8.0 / (12.0_f64.sqrt() * 10.0_f64.sqrt());
        assert_eq!(
            row.get::<_, f64>(0),
            -8.0,
            "<#> is the negative inner product"
        );
        assert!((row.get::<_, f64>(1) - expected_cosine).abs() < 1e-9);
        assert!((row.get::<_, f64>(2) - 6.0_f64.sqrt()).abs() < 1e-9);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_bind_pgvector_operators_tighter_than_comparisons() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_operator_precedence").await;

        // Act
        let within = client
            .query(
                "SELECT id FROM pgvector_semantics \
                 WHERE embedding <-> '[1,0,0]'::vector < 1.5 ORDER BY id",
                &[],
            )
            .await
            .expect("compare an L2 distance in WHERE");
        let ip_within = client
            .query(
                "SELECT id FROM pgvector_semantics \
                 WHERE embedding <#> '[3,1,0]'::vector <= -3 ORDER BY id",
                &[],
            )
            .await
            .expect("compare a negative inner product in WHERE");

        // Assert
        assert_eq!(row_ids(&within), vec![1, 3]);
        assert_eq!(row_ids(&ip_within), vec![1, 3, 5]);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_propagate_null_through_pgvector_operators() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_operator_nulls").await;
        client
            .batch_execute("INSERT INTO pgvector_semantics VALUES (6, NULL)")
            .await
            .expect("insert a NULL vector");

        for operator in ["<->", "<#>", "<=>"] {
            // Act
            let column_null: Option<f64> = client
                .query_one(
                    &format!(
                        "SELECT embedding {operator} '[3,1,0]'::vector \
                         FROM pgvector_semantics WHERE id = 6"
                    ),
                    &[],
                )
                .await
                .expect("NULL column operand")
                .get(0);
            let literal_null: Option<f64> = client
                .query_one(
                    &format!("SELECT '[1,2,3]'::vector {operator} NULL::vector"),
                    &[],
                )
                .await
                .expect("NULL literal operand")
                .get(0);

            // Assert
            assert_eq!(column_null, None, "{operator} with a NULL column");
            assert_eq!(literal_null, None, "{operator} with a NULL literal");
        }

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_pgvector_operators_across_different_dimensions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_dimension_mismatch").await;

        for operator in ["<->", "<#>", "<=>"] {
            // Act
            let error = client
                .query_one(
                    &format!("SELECT '[1,2]'::vector {operator} '[1,2,3]'::vector"),
                    &[],
                )
                .await
                .expect_err("operands with different dimensions must be rejected");

            // Assert
            let db = db_error(&error);
            assert_eq!(db.code().code(), "22000", "{operator}: {}", db.message());
            assert!(
                db.message().contains("different vector dimensions"),
                "{operator} message: {}",
                db.message()
            );
        }

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_bind_prepared_vector_parameter_in_operator_ordering() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_prepared_parameter").await;
        let statement = client
            .prepare(
                "SELECT id FROM pgvector_semantics \
                 ORDER BY embedding <#> $1::vector, id LIMIT 3",
            )
            .await
            .expect("prepare a typed vector parameter");

        // Act
        let rows = client
            .query(&statement, &[&VectorParam(vec![3.0, 1.0, 0.0])])
            .await
            .expect("execute with a binary vector parameter");

        // Assert
        assert_eq!(statement.params()[0].name(), "vector");
        assert_eq!(row_ids(&rows), vec![5, 3, 1]);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_deferred_pgvector_surface() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let (server, client, connection) = start_fixture("pgvector_deferred_surface").await;
        let statements = [
            (
                "SELECT l2_distance(embedding, embedding) FROM pgvector_semantics",
                "42883",
            ),
            (
                "SELECT inner_product(embedding, embedding) FROM pgvector_semantics",
                "42883",
            ),
            (
                "SELECT vector_dims(embedding) FROM pgvector_semantics",
                "42883",
            ),
            (
                "SELECT vector_norm(embedding) FROM pgvector_semantics",
                "42883",
            ),
            ("SELECT '{1,2,3}'::real[]::vector", "42846"),
            ("SELECT ARRAY[1, 2, 3]::vector", "42846"),
        ];

        for (sql, expected_code) in statements {
            // Act
            let error = client
                .query(sql, &[])
                .await
                .expect_err("deferred pgvector surface must be rejected");

            // Assert
            let db = db_error(&error);
            assert_eq!(db.code().code(), expected_code, "{sql}: {}", db.message());
        }

        drop(client);
        server.shutdown(connection).await;
    });
}

fn row_ids(rows: &[tokio_postgres::Row]) -> Vec<i32> {
    rows.iter().map(|row| row.get::<_, i32>(0)).collect()
}
