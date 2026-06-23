use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_scan_scalar_index_with_signed_float_bounds() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_lexkey_numeric_bounds");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_lexkey_numeric_bounds (score INT, rating FLOAT)",
                vec![],
            )
            .unwrap();
        for (id, score, rating) in [
            ("row-1", -10, -2.5),
            ("row-2", -2, -1.25),
            ("row-3", 0, 0.5),
            ("row-4", 7, 3.75),
        ] {
            cassie
                .midge
                .put_document(
                    "scalar_lexkey_numeric_bounds",
                    Some(id.to_string()),
                    serde_json::json!({"score": score, "rating": rating}),
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX scalar_lexkey_score_idx ON scalar_lexkey_numeric_bounds USING btree (score)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX scalar_lexkey_rating_idx ON scalar_lexkey_numeric_bounds USING btree (rating)",
                vec![],
            )
            .unwrap();

        // Act
        let scores = cassie
            .execute_sql(
                &session,
                "SELECT score FROM scalar_lexkey_numeric_bounds WHERE score >= -2 AND score < 7 ORDER BY score",
                vec![],
            )
            .unwrap();
        let ratings = cassie
            .execute_sql(
                &session,
                "SELECT rating FROM scalar_lexkey_numeric_bounds WHERE rating > -2.5 AND rating <= 0.5 ORDER BY rating",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            scores.rows,
            vec![vec![Value::Int64(-2)], vec![Value::Int64(0)]]
        );
        assert_eq!(
            ratings.rows,
            vec![vec![Value::Float64(-1.25)], vec![Value::Float64(0.5)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_composite_scalar_index_with_embedded_nul_text() {
    // Arrange
    with_fallback();
    let path = data_dir("scalar_lexkey_nul_text");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE scalar_lexkey_nul_text (tenant TEXT, label TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "scalar_lexkey_nul_text",
                Some("row-1".to_string()),
                serde_json::json!({"tenant": "acme", "label": "aa"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "scalar_lexkey_nul_text",
                Some("row-2".to_string()),
                serde_json::json!({"tenant": "acme", "label": "a\u{0}a"}),
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX scalar_lexkey_tenant_label_idx ON scalar_lexkey_nul_text USING btree (tenant, label)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT label FROM scalar_lexkey_nul_text WHERE tenant = 'acme' AND label >= 'a' ORDER BY label",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("a\u{0}a".to_string())],
                vec![Value::String("aa".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=scalar_lexkey_tenant_label_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}
