use cassie::types::{Value, Vector};

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
fn should_rank_cast_vector_queries_with_native_top_k() {
    // Arrange
    with_vector_fixture("vector_cast_top_k", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE cast_vectors (embedding VECTOR(3))",
        );
        for (id, payload) in [
            ("zero", serde_json::json!({"embedding": [0, 0, 0]})),
            ("a", serde_json::json!({"embedding": [1, 0, 0]})),
            ("A", serde_json::json!({"embedding": [-1, 0, 0]})),
            ("null", serde_json::json!({"embedding": null})),
            ("missing", serde_json::json!({})),
        ] {
            cassie
                .midge
                .put_document("cast_vectors", Some(id.to_owned()), payload)
                .expect("seed exact vector candidate fixture");
        }
        let baseline = sql(
            cassie,
            session,
            "SELECT _id AS identity, vector_distance(embedding, '[0,0,0]') AS score \
             FROM cast_vectors ORDER BY score LIMIT 2 OFFSET 1",
        );
        let expected = [
            vec![Value::String("A".into()), Value::Float64(1.0)],
            vec![Value::String("a".into()), Value::Float64(1.0)],
        ];

        // Act
        for (operand, order, params) in [
            ("'[0,0,0]'::vector", "score", vec![]),
            (
                "$1::vector",
                "vector_distance(embedding, $1::vector(3))",
                vec![Value::String("[0,0,0]".into())],
            ),
            (
                "$1::vector(3)",
                "score",
                vec![Value::Vector(Vector::new(vec![0.0, 0.0, 0.0]))],
            ),
        ] {
            let query = format!(
                "SELECT _id AS identity, vector_distance(embedding, {operand}) AS score \
                 FROM cast_vectors ORDER BY {order} LIMIT 2 OFFSET 1"
            );
            let rows = cassie
                .execute_sql(session, &query, params.clone())
                .expect("rank candidates with explicit vector cast");
            let explain = cassie
                .execute_sql(session, &format!("EXPLAIN ANALYZE {query}"), params)
                .expect("analyze the executed top-k path");

            // Assert
            assert_eq!(rows.rows(), expected.as_slice());
            assert_eq!(rows.columns(), baseline.columns());
            let diagnostics = explain.rows()[0][0].as_str().expect("text explain plan");
            assert!(diagnostics.contains("actual_rows=2"), "{diagnostics}");
            assert!(
                diagnostics.contains("candidate_count_delta:3,"),
                "{diagnostics}"
            );
            assert!(
                diagnostics.contains("result_count_delta:2,"),
                "{diagnostics}"
            );
        }
    });
}

#[test]
fn should_preserve_cast_validation_in_vector_top_k() {
    // Arrange
    with_vector_fixture("vector_cast_validation", |cassie, session| {
        seed(cassie, session, "vector_cast_validation");

        // Act
        let wrong_field_width = cassie
            .execute_sql(
                session,
                "SELECT _id, vector_distance(embedding, '[1,0]'::vector(2)) AS score \
                 FROM vector_cast_validation ORDER BY score LIMIT 0",
                vec![],
            )
            .expect_err("zero limit must retain vector field validation");
        let wrong_cast_width = cassie
            .execute_sql(
                session,
                "SELECT _id, vector_distance(embedding, $1::vector(2)) AS score \
                 FROM vector_cast_validation ORDER BY score LIMIT 0",
                vec![Value::Vector(Vector::new(vec![1.0, 0.0, 0.0]))],
            )
            .expect_err("zero limit must retain explicit vector typmod validation");
        let intervening_cast = cassie
            .execute_sql(
                session,
                "SELECT _id, vector_distance(embedding, '[1,0,0]'::varchar(1)::vector) AS score \
                 FROM vector_cast_validation ORDER BY score LIMIT 1",
                vec![],
            )
            .expect_err("the intervening VARCHAR cast truncates the input");

        // Assert
        assert!(wrong_field_width
            .to_string()
            .contains("expects 3 dimensions"));
        for error in [wrong_cast_width, intervening_cast] {
            assert_eq!(error.sql_state(), "22000");
            assert!(
                error.to_string().contains("invalid shape or dimension"),
                "{error}"
            );
        }
    });
}

#[test]
fn should_preserve_finite_boundary_in_cast_typed_vector_query() {
    // Arrange
    with_vector_fixture("vector_cast_finite_boundary", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE cast_vector_boundary (embedding VECTOR(3))",
        );
        sql(
            cassie,
            session,
            "INSERT INTO cast_vector_boundary VALUES ('[0,0,0]'::vector)",
        );

        // Act
        let result = cassie
            .execute_sql(
                session,
                "SELECT _id, vector_distance(embedding, $1::vector) AS score \
                 FROM cast_vector_boundary ORDER BY score LIMIT 1",
                vec![Value::Vector(Vector::new(vec![f32::MAX, 0.0, 0.0]))],
            )
            .expect("score the maximum finite vector component in the native carrier");

        // Assert
        assert_eq!(result.row_count(), 1);
        assert_eq!(result.rows()[0][1], Value::Float64(f64::from(f32::MAX)));
    });
}

#[test]
fn should_respect_distinct_ordering_vector_and_cast_in_top_k() {
    // Arrange
    with_vector_fixture("vector_cast_distinct_order", |cassie, session| {
        seed(cassie, session, "vector_cast_distinct_order");
        let query = "SELECT _id, vector_distance(embedding, '[1,0,0]'::vector) AS score \
                     FROM vector_cast_distinct_order ORDER BY vector_distance(embedding, ";

        // Act
        let distinct = sql(
            cassie,
            session,
            &format!("{query}'[0,1,0]'::vector) LIMIT 1"),
        );
        let invalid_order = cassie
            .execute_sql(
                session,
                &format!("{query}$1::vector(2)) LIMIT 1"),
                vec![Value::String("[1,0,0]".into())],
            )
            .expect_err("ORDER BY must validate its own cast");

        // Assert
        assert_eq!(
            distinct.rows(),
            &[vec![
                Value::String("b".into()),
                Value::Float64(2.0_f64.sqrt())
            ]]
        );
        assert_eq!(invalid_order.sql_state(), "22000");
        assert!(invalid_order
            .to_string()
            .contains("invalid shape or dimension"));
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
