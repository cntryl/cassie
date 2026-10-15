use super::support_read_equivalence::{sql, with_fixture as with_vector_fixture};

fn seed(cassie: &cassie::app::Cassie, session: &cassie::app::CassieSession, table: &str) {
    sql(
        cassie,
        session,
        &format!("CREATE TABLE {table} (body TEXT,embedding VECTOR(3))"),
    );
    for (id, vector) in [
        ("a", [1.0, 0.0, 0.0]),
        ("b", [0.0, 1.0, 0.0]),
        ("c", [0.0, 0.0, 1.0]),
    ] {
        cassie
            .midge
            .put_document(
                table,
                Some(id.to_string()),
                serde_json::json!({"body":"alpha","embedding":vector}),
            )
            .expect("seed vector");
    }
}

#[test]
fn should_preserve_vector_top_k_identity_output_alias() {
    // Arrange
    with_vector_fixture("vector_identity_alias", |cassie, session| {
        seed(cassie, session, "vector_alias");
        // Act
        for projection in [
            "id,vector_distance(embedding,'[1,0,0]')",
            "_id AS identity,vector_distance(embedding,'[1,0,0]') AS distance",
        ] {
            let ordering = if projection.contains("AS distance") {
                "distance"
            } else {
                "vector_distance(embedding,'[1,0,0]')"
            };
            let query = format!("SELECT {projection} FROM vector_alias ORDER BY {ordering} ASC");
            let baseline = sql(cassie, session, &query);
            let bounded = sql(cassie, session, &format!("{query} LIMIT 2"));
            // Assert
            assert_eq!(bounded.columns().len(), 2);
            assert!(bounded
                .rows()
                .iter()
                .all(|row| row.len() == bounded.columns().len()));
            assert_eq!(bounded.rows(), &baseline.rows()[..2]);
        }
    });
}

#[test]
fn should_preserve_zero_limit_in_vector_top_k() {
    // Arrange
    with_vector_fixture("vector_limit_zero", |cassie, session| {
        seed(cassie, session, "vector_zero");
        let invalid = cassie.execute_sql(session,
                "SELECT _id,vector_distance(embedding,'[1,0]') AS distance FROM vector_zero ORDER BY distance ASC LIMIT 0", vec![])
                .expect_err("zero limit preserves vector dimension validation");
        assert!(
            invalid.to_string().contains("expects 3 dimensions"),
            "{invalid}"
        );
        // Act
        for tail in ["LIMIT 0", "LIMIT 0 OFFSET 1"] {
            let result=sql(cassie,session,&format!("SELECT _id,vector_distance(embedding,'[1,0,0]') AS distance FROM vector_zero ORDER BY distance ASC {tail}"));
            // Assert
            assert!(result.rows().is_empty(), "{tail}");
        }
    });
}

#[test]
fn should_preserve_zero_limit_in_hybrid_top_k() {
    // Arrange
    with_vector_fixture("hybrid_limit_zero", |cassie, session| {
        seed(cassie, session, "hybrid_zero");
        sql(
            cassie,
            session,
            "CREATE INDEX hybrid_zero_body ON hybrid_zero USING fulltext(body)",
        );
        // Act
        for tail in ["LIMIT 0", "LIMIT 0 OFFSET 1"] {
            let result=sql(cassie,session,&format!("SELECT _id,hybrid_score(search_score(body,'alpha'),vector_score(embedding,'[1,0,0]')) AS score FROM hybrid_zero ORDER BY score DESC {tail}"));
            // Assert
            assert!(result.rows().is_empty(), "{tail}");
        }
    });
}

#[test]
fn should_not_record_ivfflat_fallback_without_an_index() {
    // Arrange
    with_vector_fixture("unindexed_vector_descending_metrics", |cassie, session| {
        seed(cassie, session, "vector_no_ann");
        let before = cassie.metrics()["vector"]["ivfflat_fallbacks"].clone();
        // Act
        let result = sql(cassie, session,
                "SELECT _id,vector_distance(embedding,'[1,0,0]') AS distance FROM vector_no_ann ORDER BY distance DESC LIMIT 1");
        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(cassie.metrics()["vector"]["ivfflat_fallbacks"], before);
    });
}

#[test]
fn should_preserve_descending_vector_order_with_ivfflat_indexes() {
    // Arrange
    with_vector_fixture("ivfflat_descending_equivalence", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE ivf_descending (body TEXT,embedding VECTOR(3))",
        );
        for index in 0..20 {
            let coordinate = if index % 2 == 0 { 1.0 } else { -1.0 };
            cassie
                .midge
                .put_document(
                    "ivf_descending",
                    Some(format!("row-{index:02}")),
                    serde_json::json!({"body":"alpha","embedding":[coordinate,0.0,0.0]}),
                )
                .expect("seed clustered vectors");
        }
        let query="SELECT _id,vector_distance(embedding,'[1,0,0]') AS distance FROM ivf_descending ORDER BY distance DESC LIMIT 4";
        let baseline = sql(cassie, session, query);
        sql(cassie,session,"CREATE INDEX ivf_descending_idx ON ivf_descending USING vector(embedding) WITH (source_field = body,metric = l2,index_type = ivfflat,lists = 2,probes = 1,training_sample_size = 20,training_seed = 17)");
        // Restore explicit vectors after provider-backed index creation.
        for index in 0..20 {
            let coordinate = if index % 2 == 0 { 1.0 } else { -1.0 };
            cassie
                .midge
                .put_document(
                    "ivf_descending",
                    Some(format!("row-{index:02}")),
                    serde_json::json!({"body":"alpha","embedding":[coordinate,0.0,0.0]}),
                )
                .expect("restore clustered vectors");
        }
        let training = cassie
            .midge
            .get_vector_index("ivf_descending", "embedding")
            .expect("index metadata")
            .expect("index")
            .metadata
            .ivfflat_training
            .expect("trained index");
        assert!(
            training.list_sizes.iter().all(|size| *size > 0),
            "{training:?}"
        );
        let ascending = sql(cassie, session, "SELECT _id,vector_distance(embedding,'[1,0,0]') AS distance FROM ivf_descending ORDER BY distance ASC LIMIT 4");
        assert_eq!(ascending.rows().len(), 4);
        let metrics = cassie.metrics();
        assert!(
            metrics["vector"]["ivfflat_executions"].as_u64().unwrap() > 0,
            "{metrics}"
        );
        // Act
        let indexed = sql(cassie, session, query);
        // Assert
        assert_eq!(indexed.rows(), baseline.rows());
    });
}

#[test]
fn should_preserve_zero_limit_in_rest_vector_search_execution() {
    // Arrange
    with_vector_fixture("rest_vector_limit_zero", |cassie, session| {
        seed(cassie, session, "rest_vector_zero");
        sql(cassie,session,"CREATE INDEX rest_vector_zero_idx ON rest_vector_zero USING vector(embedding) WITH (source_field = body,metric = l2)");
        // Act
        let result = cassie
            .execute_vector_search("rest_vector_zero", "embedding", "alpha", None, 0, 0)
            .expect("REST vector search executor");
        // Assert
        assert_eq!(result.rows(), [] as [Vec<cassie::types::Value>; 0]);
        assert_ne!(result.columns(), [] as [cassie::executor::ColumnMeta; 0]);
    });
}
