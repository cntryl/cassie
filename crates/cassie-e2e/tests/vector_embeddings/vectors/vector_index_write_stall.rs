// Bulk vector index builds retry Midge write-stall backpressure instead of failing the build.
use cassie::midge::adapter::{
    document_write_storage_failures_remaining, set_document_write_storage_failures,
    DocumentWriteStorageFailure,
};

use super::support_read_equivalence::{sql, with_fixture};

fn seed(cassie: &cassie::app::Cassie, session: &cassie::app::CassieSession, table: &str) {
    sql(
        cassie,
        session,
        &format!("CREATE TABLE {table} (body TEXT, embedding VECTOR(3))"),
    );
    let documents = (0..64_u8)
        .map(|row| {
            let angle = f32::from(row) / 10.0;
            (
                Some(format!("row-{row:02}")),
                serde_json::json!({"body": "alpha", "embedding": [angle.cos(), angle.sin(), 1.0]}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, documents)
        .expect("seed vector rows");
}

#[test]
fn should_build_an_hnsw_index_through_transient_write_stalls() {
    // Arrange
    with_fixture("vector_index_write_stall", |cassie, session| {
        seed(cassie, session, "write_stall_rows");
        set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 3);

        // Act
        let result = cassie.execute_sql(
            session,
            "CREATE INDEX write_stall_idx ON write_stall_rows USING vector (embedding) \
             WITH (source_field = body, metric = l2, index_type = hnsw, m = 16, \
             ef_construction = 64, ef_search = 64)",
            vec![],
        );
        let unconsumed = document_write_storage_failures_remaining();
        set_document_write_storage_failures(DocumentWriteStorageFailure::WriteStall, 0);

        // Assert
        assert!(
            result.is_ok(),
            "the index build did not retry a write stall"
        );
        assert_eq!(unconsumed, 0, "the injected write stalls were not all met");
        assert!(cassie
            .midge
            .get_vector_index("write_stall_rows", "embedding")
            .expect("read vector index metadata")
            .is_some());
    });
}
