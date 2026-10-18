use super::*;

#[derive(Debug, PartialEq)]
struct VectorCatalogObservation {
    extension: String,
    operators: Vec<(String, i32, i32, String)>,
    opclasses: Vec<(String, String)>,
}

async fn read_vector_catalog_observation(
    client: &tokio_postgres::Client,
    vector_oid: i32,
) -> VectorCatalogObservation {
    let extension = client
        .query_one(
            "SELECT extversion FROM pg_catalog.pg_extension WHERE extname = 'vector'",
            &[],
        )
        .await
        .expect("read the installed extension version")
        .get(0);
    let operators = client
        .query(
            "SELECT operator.oprname, operator.oprleft::INT, \
                    operator.oprright::INT, operator.oprresult::regtype::text \
             FROM pg_catalog.pg_operator AS operator \
             WHERE operator.oprleft = $1::OID AND operator.oprright = $1::OID \
               AND operator.oprname IN ('<->', '<#>', '<=>', '<+>') \
             ORDER BY operator.oprname",
            &[&vector_oid],
        )
        .await
        .expect("read vector metric operators by their dynamic operand OID");
    let opclasses = client
        .query(
            "SELECT access_method.amname, opclass.opcname \
             FROM pg_catalog.pg_opclass AS opclass \
             JOIN pg_catalog.pg_am AS access_method \
               ON access_method.oid = opclass.opcmethod \
             WHERE opclass.opcintype = $1::OID \
               AND opclass.opcname IN ( \
                 'vector_l2_ops', 'vector_ip_ops', \
                 'vector_cosine_ops', 'vector_l1_ops') \
             ORDER BY access_method.amname, opclass.opcname",
            &[&vector_oid],
        )
        .await
        .expect("read supported vector operator classes by their dynamic type OID");
    VectorCatalogObservation {
        extension,
        operators: operators
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, i32>(2),
                    row.get::<_, String>(3),
                )
            })
            .collect(),
        opclasses: opclasses
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect(),
    }
}

#[test]
fn should_report_vector_catalog_identity_given_the_pinned_extension_when_catalogs_are_queried() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, vector_oid) =
            start_vector_server("pgvector_operator_catalogs").await;

        // Act
        let observation = read_vector_catalog_observation(&client, vector_oid).await;

        // Assert
        assert_eq!(observation.extension, "0.8.7");
        assert_eq!(
            observation.operators,
            vec![
                (
                    "<#>".to_owned(),
                    vector_oid,
                    vector_oid,
                    "double precision".to_owned(),
                ),
                (
                    "<+>".to_owned(),
                    vector_oid,
                    vector_oid,
                    "double precision".to_owned(),
                ),
                (
                    "<->".to_owned(),
                    vector_oid,
                    vector_oid,
                    "double precision".to_owned(),
                ),
                (
                    "<=>".to_owned(),
                    vector_oid,
                    vector_oid,
                    "double precision".to_owned(),
                ),
            ]
        );
        assert_eq!(
            observation.opclasses,
            vec![
                ("hnsw".to_owned(), "vector_cosine_ops".to_owned()),
                ("hnsw".to_owned(), "vector_ip_ops".to_owned()),
                ("hnsw".to_owned(), "vector_l1_ops".to_owned()),
                ("hnsw".to_owned(), "vector_l2_ops".to_owned()),
                ("ivfflat".to_owned(), "vector_cosine_ops".to_owned()),
                ("ivfflat".to_owned(), "vector_ip_ops".to_owned()),
                ("ivfflat".to_owned(), "vector_l2_ops".to_owned()),
            ]
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
