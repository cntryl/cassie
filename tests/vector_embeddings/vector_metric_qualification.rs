use super::vector_qualification_support::{fixture, oracle};
use cassie::types::Value;
use std::sync::atomic::Ordering;

#[test]
fn should_refresh_large_warm_normalized_cache_after_vector_generation_changes() {
    // Arrange
    let (cassie, path, _) = fixture("dot", Some("index_type = bruteforce"));
    let documents = (0..1020)
        .map(|index| {
            let id = format!("id{index:04}");
            (
                Some(id.clone()),
                serde_json::json!({"id":id,"content":null,"embedding":[1.0,0.0,0.0]}),
            )
        })
        .collect();
    cassie
        .midge
        .put_documents("vectors", documents)
        .expect("large explicit population");
    let collection = cassie
        .catalog
        .get_schema("vectors")
        .expect("schema")
        .collection;
    let stats = cassie
        .midge
        .rebuild_cardinality_stats_for_collection(&collection)
        .expect("current stats");
    cassie.catalog.set_cardinality_stats(&collection, stats);
    let body = br#"{"field":"embedding","query":"unit","metric":"dot","limit":1}"#;
    for _ in 0..2 {
        let warm =
            cassie::rest::search::vector_search(&cassie, "vectors", body).expect("warm search");
        assert_eq!(warm.rows[0][0].as_str(), Some("id0000"));
    }
    assert!(
        cassie.metrics()["vector"]["normalized_candidate_count_total"]
            .as_u64()
            .expect("normalized candidates")
            >= 2050
    );
    let catalog_version = cassie.catalog.version();
    cassie
        .midge
        .put_document(
            "vectors",
            Some("id0500".into()),
            serde_json::json!({"id":"id0500","content":null,"embedding":[2.0,0.0,0.0]}),
        )
        .expect("advance vector generation");
    assert!(cassie
        .midge
        .get_cardinality_stats(&collection)
        .expect("stale stats")
        .is_none());

    // Act
    let stale = cassie::rest::search::vector_search(&cassie, "vectors", body)
        .expect("stale-statistics fallback");
    cassie
        .midge
        .delete_cardinality_stats(&collection)
        .expect("remove statistics");
    let missing = cassie::rest::search::vector_search(&cassie, "vectors", body)
        .expect("missing-statistics fallback");
    let current = cassie
        .midge
        .rebuild_cardinality_stats_for_collection(&collection)
        .expect("rebuild same population");
    assert!(current.hydrated);
    assert_eq!(
        current
            .field_stats("embedding")
            .expect("vector population")
            .non_null_count,
        1025
    );
    assert_eq!(cassie.catalog.version(), catalog_version);

    let refreshed = cassie::rest::search::vector_search(&cassie, "vectors", body)
        .expect("generation-aware warm search");
    cassie.catalog.set_cardinality_stats(&collection, current);
    let cold = cassie::rest::search::vector_search(&cassie, "vectors", body).expect("cold control");

    // Assert
    assert_eq!(cold.rows[0][0].as_str(), Some("id0500"));
    assert_eq!(
        serde_json::to_value(&refreshed.columns).expect("warm descriptors"),
        serde_json::to_value(&cold.columns).expect("cold descriptors")
    );
    assert_eq!(refreshed.rows, cold.rows);
    assert_eq!(stale.rows, cold.rows);
    assert_eq!(missing.rows, cold.rows);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_match_zero_vector_order_across_query_paths() {
    // Arrange
    for (metric, operator) in [("l2", "<->"), ("cosine", "<=>"), ("dot", "<#>")] {
        for options in [None, Some("index_type = bruteforce"), Some("index_type = hnsw, m = 4, ef_construction = 16, ef_search = 16"), Some("index_type = ivfflat, lists = 2, probes = 2, training_sample_size = 5, training_seed = 17")] {
            let (cassie, path, provider) = fixture(metric, options);
            let session = cassie.create_session("tester", None);
            // Act
            for (name, query) in [("zero", vec![0.0;3]), ("unit", vec![1.0,0.0,0.0])] {
                for descending in [false, true] {
                    let expected = oracle(metric, &query, descending);
                    let direction = if descending { "DESC" } else { "ASC" };
                    let result = cassie.execute_sql(&session, &format!("SELECT _id, embedding {operator} $1 AS distance FROM vectors WHERE embedding IS NOT NULL ORDER BY distance {direction} LIMIT 5"), vec![Value::Vector(cassie::types::Vector::new(query.clone()))]).expect("SQL");
                    // Assert
                    assert_eq!(result.rows.len(), expected.len(), "{metric} {options:?} {name}");
                    for (row,(id,distance)) in result.rows.iter().zip(&expected) {
                        assert_eq!(row[0], Value::String(id.clone()), "{metric} {options:?} {name} {direction}");
                        let Value::Float64(actual) = row[1] else { panic!("distance must be f64") };
                        assert!((actual-distance).abs() < 1e-12, "{actual} != {distance}");
                    }
                }
                if options.is_some() {
                    let body = serde_json::to_vec(&serde_json::json!({"field":"embedding","query":name,"metric":metric,"limit":5})).expect("body");
                    let result = cassie::rest::search::vector_search(&cassie,"vectors",&body).expect("REST");
                    let expected = oracle(metric,&query,false);
                    let identity = result.columns.iter().position(|column| column.name == "_id").expect("identity");
                    assert_eq!(result.rows.iter().map(|row| row[identity].as_str().expect("id")).collect::<Vec<_>>(), expected.iter().map(|row| row.0.as_str()).collect::<Vec<_>>(), "REST {metric} {options:?} {name}");
                }
            }
            if metric == "l2" {
                let result = cassie.execute_sql(&session,"SELECT _id, vector_distance(embedding, $1) AS distance FROM vectors ORDER BY distance ASC LIMIT 5",vec![Value::Vector(cassie::types::Vector::new(vec![0.0;3]))]).expect("native L2 SQL");
                assert_eq!(result.rows.iter().map(|row| row[0].as_str().expect("identity")).collect::<Vec<_>>(),oracle(metric,&[0.0;3],false).iter().map(|row| row.0.as_str()).collect::<Vec<_>>());
            }
            if let Some(options) = options {
                if metric != "l2" && !options.contains("hnsw") {
                    assert!(cassie.metrics()["vector"]["normalized_candidate_count_total"].as_u64().expect("normalized path counter") >= 10);
                }
                if options.contains("hnsw") { assert!(cassie.metrics()["vector"]["hnsw_executions"].as_u64().expect("HNSW counter") > 0); }
                if options.contains("ivfflat") && metric == "l2" { assert!(cassie.metrics()["vector"]["ivfflat_executions"].as_u64().expect("IVF counter") > 0); }
                let before = provider.0.load(Ordering::SeqCst);
                let body = br#"{"field":"embedding","query":"unavailable","limit":0}"#;
                assert_eq!(cassie::rest::search::vector_search(&cassie,"vectors",body).expect("zero REST").rows, Vec::<Vec<serde_json::Value>>::new());
                assert_eq!(provider.0.load(Ordering::SeqCst),before);
            }
            assert_eq!(cassie.metrics()["query"]["current_accounted_memory_bytes"],0);
            drop(cassie);
            std::fs::remove_dir_all(path).expect("cleanup");
        }
    }
}

#[test]
fn should_match_independent_metric_oracles_at_dispatch_boundaries() {
    // Arrange
    for dimensions in [1, 3, 4, 8, 9, 17, 1025] {
        for magnitude in [f32::from_bits(1), 1.0, f32::MAX] {
            let query = vec![magnitude; dimensions];
            let target = vec![-magnitude; dimensions];
            let dot: f64 = query
                .iter()
                .zip(&target)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            let l2 = query
                .iter()
                .zip(&target)
                .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                .sum::<f64>()
                .sqrt();
            // Act
            let actual = [
                cassie::vector::dot_distance(&query, &target),
                cassie::vector::l2_distance(&query, &target),
                cassie::vector::cosine_distance(&query, &target),
            ];
            // Assert
            for (result, expected) in actual.into_iter().zip([-dot, l2, 2.0]) {
                assert!(result.is_finite());
                assert!(
                    (result - expected).abs() <= expected.abs() * 1e-12,
                    "dims={dimensions}, magnitude={magnitude}, {result} != {expected}"
                );
            }
        }
    }
}

#[test]
fn should_validate_all_vector_operator_dimensions_before_zero_limit() {
    // Arrange
    let (cassie, path, _) = fixture("l2", None);
    let session = cassie.create_session("tester", None);
    // Act
    for operator in ["<->", "<=>", "<#>"] {
        for value in [
            Value::String("[0,0]".into()),
            Value::Vector(cassie::types::Vector::new(vec![0.0; 2])),
        ] {
            let error = cassie.execute_sql(&session,&format!("SELECT _id, embedding {operator} $1 AS distance FROM vectors ORDER BY distance LIMIT 0"),vec![value]).expect_err("dimension mismatch");
            // Assert
            assert!(
                error.to_string().contains("dimension"),
                "{operator}: {error}"
            );
        }
    }
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_match_zero_vector_oracle_when_rest_normalized_sidecars_are_missing() {
    // Arrange
    for metric in ["l2", "cosine", "dot"] {
        let (cassie, path, _) = fixture(metric, Some("index_type = bruteforce"));
        let collection = cassie
            .catalog
            .get_schema("vectors")
            .expect("schema")
            .collection;
        super::support_sql::clear_normalized_sidecars(&cassie, &collection, "embedding");
        // Act
        for (name, query) in [("zero", vec![0.0; 3]), ("unit", vec![1.0, 0.0, 0.0])] {
            let body = serde_json::to_vec(
                &serde_json::json!({"field":"embedding","query":name,"metric":metric,"limit":5}),
            )
            .expect("body");
            let result =
                cassie::rest::search::vector_search(&cassie, "vectors", &body).expect("raw REST");
            let identity = result
                .columns
                .iter()
                .position(|column| column.name == "_id")
                .expect("identity");
            // Assert
            assert_eq!(
                result
                    .rows
                    .iter()
                    .map(|row| row[identity].as_str().expect("id"))
                    .collect::<Vec<_>>(),
                oracle(metric, &query, false)
                    .iter()
                    .map(|row| row.0.as_str())
                    .collect::<Vec<_>>()
            );
        }
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"],
            0
        );
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_search_explicit_sql_vectors_without_sources_when_normalized_sidecars_are_missing() {
    // Arrange
    for metric in ["cosine", "dot"] {
        let (cassie, path, _) = fixture(metric, None);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "DELETE FROM vectors", vec![])
            .expect("clear seed");
        cassie.execute_sql(&session,&format!("CREATE INDEX qualified_sql_vector ON vectors USING vector(embedding) WITH (source_field = content, metric = {metric})"),vec![]).expect("index");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO vectors (id, content, embedding) VALUES ('sql-explicit', NULL, $1)",
                vec![Value::Vector(cassie::types::Vector::new(vec![0.0; 3]))],
            )
            .expect("explicit SQL vector");
        let collection = cassie
            .catalog
            .get_schema("vectors")
            .expect("schema")
            .collection;
        let stored = cassie
            .execute_sql(&session, "SELECT embedding FROM vectors", vec![])
            .expect("stored SQL vector");
        assert_eq!(
            stored.rows,
            vec![vec![Value::Vector(cassie::types::Vector::new(vec![
                0.0;
                3
            ]))]]
        );
        super::support_sql::clear_normalized_sidecars(&cassie, &collection, "embedding");
        // Act
        let body = serde_json::to_vec(
            &serde_json::json!({"field":"embedding","query":"zero","metric":metric,"limit":5}),
        )
        .expect("body");
        let result = cassie::rest::search::vector_search(&cassie, "vectors", &body).expect("REST");
        // Assert
        assert_eq!(
            result.rows.len(),
            1,
            "{metric} must find explicit SQL vector despite absent source"
        );
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"],
            0
        );
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_preserve_restarted_vector_ordering() {
    // Arrange
    for (metric, operator) in [("l2", "<->"), ("cosine", "<=>"), ("dot", "<#>")] {
        let (cassie, path, _) = fixture(metric, Some("index_type = bruteforce"));
        let query = format!("SELECT _id, embedding {operator} '[0,0,0]' AS distance FROM vectors WHERE embedding IS NOT NULL ORDER BY distance ASC LIMIT 2 OFFSET 1");
        let expected = oracle(metric, &[0.0; 3], false)[1..3]
            .iter()
            .map(|row| row.0.clone())
            .collect::<Vec<_>>();
        // Act
        let before = cassie
            .execute_sql(&cassie.create_session("tester", None), &query, vec![])
            .expect("before restart");
        drop(cassie);
        let cassie = super::vector_qualification_support::reopen(&path);
        let after = cassie
            .execute_sql(&cassie.create_session("tester", None), &query, vec![])
            .expect("after restart");
        // Assert
        assert_eq!(before.rows, after.rows);
        assert_eq!(
            after
                .rows
                .iter()
                .map(|row| row[0].as_str().expect("id").to_owned())
                .collect::<Vec<_>>(),
            expected
        );
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_match_vector_expression_order_on_generic_sql_path() {
    // Arrange
    let (cassie, path, _) = fixture(
        "l2",
        Some("index_type = hnsw, m = 4, ef_construction = 16, ef_search = 16"),
    );
    let session = cassie.create_session("tester", None);
    let expected = oracle("l2", &[-1.0, 0.0, 0.0], false);
    // Act
    let result = cassie.execute_sql(&session,"SELECT _id, vector_distance(embedding,'[1,0,0]') AS distance FROM vectors WHERE embedding IS NOT NULL ORDER BY vector_distance(embedding,'[-1,0,0]') LIMIT 5",vec![]).expect("distinct ordering vector");
    let direct = cassie.execute_sql(&session,"SELECT _id, embedding <-> '[-1,0,0]' AS distance FROM vectors WHERE embedding IS NOT NULL ORDER BY embedding <-> '[-1,0,0]' LIMIT 5",vec![]).expect("direct operator");
    // Assert
    for result in [&result, &direct] {
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row[0].as_str().expect("id"))
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|row| row.0.as_str())
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(cassie.metrics()["vector"]["hnsw_executions"], 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_fall_back_to_authoritative_vectors_for_partial_sidecars_or_missing_statistics() {
    // Arrange
    for metric in ["cosine", "dot"] {
        for statistics in ["present", "missing", "stale"] {
            let (cassie, path, _) = fixture(metric, Some("index_type = bruteforce"));
            let collection = cassie
                .catalog
                .get_schema("vectors")
                .expect("schema")
                .collection;
            if statistics == "missing" {
                cassie
                    .midge
                    .delete_cardinality_stats(&collection)
                    .expect("remove stats");
            } else if statistics == "stale" {
                cassie
                    .midge
                    .put_document(
                        "vectors",
                        Some("positive".into()),
                        serde_json::json!({"id":"declared-positive","embedding":[1.0,0.0,0.0]}),
                    )
                    .expect("advance source generation without refreshing catalog statistics");
                assert!(cassie
                    .midge
                    .get_cardinality_stats(&collection)
                    .expect("generation checked statistics")
                    .is_none());
            } else {
                super::vector_qualification_support::remove_one_normalized_sidecar(
                    &cassie,
                    &collection,
                );
            }
            let before = cassie.metrics();
            // Act
            let body = serde_json::to_vec(
                &serde_json::json!({"field":"embedding","query":"zero","metric":metric,"limit":5}),
            )
            .expect("body");
            let result = cassie::rest::search::vector_search(&cassie, "vectors", &body)
                .expect("fallback REST");
            let after = cassie.metrics();
            // Assert
            assert_eq!(result.rows.len(), 5);
            assert_eq!(
                after["vector"]["normalized_fallback_count_total"]
                    .as_u64()
                    .expect("fallback counter")
                    - before["vector"]["normalized_fallback_count_total"]
                        .as_u64()
                        .expect("fallback counter"),
                5
            );
            assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
            drop(cassie);
            std::fs::remove_dir_all(path).expect("cleanup");
        }
    }
}

#[test]
fn should_score_all_vectors_when_partial_sidecar_count_matches_source_cardinality() {
    // Arrange
    for metric in ["cosine", "dot"] {
        let (cassie, path, _) = fixture(metric, Some("index_type = bruteforce"));
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "UPDATE vectors SET content = 'unit' WHERE id = 'declared-positive'",
                vec![],
            )
            .expect("one sourced vector plus four explicit vectors");
        let collection = cassie
            .catalog
            .get_schema("vectors")
            .expect("schema")
            .collection;
        cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&collection)
            .expect("rebuild current witness statistics");
        let statistics = cassie
            .midge
            .get_cardinality_stats(&collection)
            .expect("current stats")
            .expect("statistics");
        assert_eq!(
            statistics.index_cardinality(
                &cassie::catalog::CollectionCardinalityStats::vector_index_key("embedding")
            ),
            Some(1)
        );
        assert_eq!(
            statistics
                .field_stats("embedding")
                .expect("vector stats")
                .non_null_count,
            5
        );
        super::vector_qualification_support::keep_one_normalized_sidecar(&cassie, &collection);
        // Act
        let body = serde_json::to_vec(
            &serde_json::json!({"field":"embedding","query":"zero","metric":metric,"limit":5}),
        )
        .expect("body");
        let result = cassie::rest::search::vector_search(&cassie, "vectors", &body).expect("REST");
        let identity = result
            .columns
            .iter()
            .position(|column| column.name == "_id")
            .expect("identity");
        // Assert
        assert_eq!(
            result
                .rows
                .iter()
                .map(|row| row[identity].as_str().expect("id"))
                .collect::<Vec<_>>(),
            oracle(metric, &[0.0; 3], false)
                .iter()
                .map(|row| row.0.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"],
            0
        );
        drop(cassie);
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_preserve_vector_window_boundary_contract() {
    // Arrange
    let (cassie, path, _) = fixture("l2", Some("index_type = bruteforce"));
    let session = cassie.create_session("tester", None);
    // Act
    for offset in [5, 8] {
        let result = cassie.execute_sql(&session,&format!("SELECT _id, vector_distance(embedding,'[0,0,0]') AS distance FROM vectors ORDER BY distance LIMIT 1 OFFSET {offset}"),vec![]).expect("empty SQL window");
        let body = serde_json::to_vec(
            &serde_json::json!({"field":"embedding","query":"zero","limit":1,"offset":offset}),
        )
        .expect("body");
        let rest = cassie::rest::search::vector_search(&cassie, "vectors", &body)
            .expect("empty REST window");
        // Assert
        assert_eq!(result.rows, Vec::<Vec<Value>>::new());
        assert_eq!(rest.rows, Vec::<Vec<serde_json::Value>>::new());
    }
    let body = serde_json::to_vec(
        &serde_json::json!({"field":"embedding","query":"zero","limit":1,"offset":usize::MAX}),
    )
    .expect("overflow body");
    assert!(cassie::rest::search::vector_search(&cassie, "vectors", &body).is_err());
    assert!(cassie.execute_sql(&session,"SELECT _id, vector_distance(embedding,'[0,0,0]') AS distance FROM vectors ORDER BY distance LIMIT 9223372036854775807 OFFSET 9223372036854775807",vec![]).is_err());
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}
