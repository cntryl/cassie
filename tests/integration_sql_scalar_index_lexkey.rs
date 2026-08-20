use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_scan_scalar_index_with_signed_float_bounds() {
    // Arrange
    use_local_storage();
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
fn should_match_integer_shaped_literals_in_float_scalar_indexes() {
    // Arrange
    use_local_storage();
    let path = data_dir("scalar_lexkey_whole_float");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        let session = cassie.create_session("tester", None);
        for table in ["whole_float_baseline", "whole_float_indexed"] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (row_number INT, rating FLOAT)"),
                    vec![],
                )
                .expect("create float table");
            cassie
                .execute_sql(
                    &session,
                    &format!("INSERT INTO {table} (row_number, rating) VALUES (1, 100.0)"),
                    vec![],
                )
                .expect("insert whole-number float");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX whole_float_rating_idx ON whole_float_indexed USING btree (rating)",
                vec![],
            )
            .expect("create float scalar index");

        // Act
        let baseline_integer = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_baseline WHERE rating = 100",
                vec![],
            )
            .expect("query unindexed float with integer literal");
        let indexed_integer = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                vec![],
            )
            .expect("query indexed float with integer literal");
        let baseline_float = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_baseline WHERE rating = 100.0",
                vec![],
            )
            .expect("query unindexed float with float literal");
        let indexed_float = cassie
            .execute_sql(
                &session,
                "SELECT row_number FROM whole_float_indexed WHERE rating = 100.0",
                vec![],
            )
            .expect("query indexed float with float literal");
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT row_number FROM whole_float_indexed WHERE rating = 100",
                vec![],
            )
            .expect("explain float scalar index lookup");

        // Assert
        let expected = vec![vec![Value::Int64(1)]];
        assert_eq!(baseline_integer.rows, expected);
        assert_eq!(indexed_integer.rows, baseline_integer.rows);
        assert_eq!(baseline_float.rows, expected);
        assert_eq!(indexed_float.rows, baseline_float.rows);
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=whole_float_rating_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_scan_composite_scalar_index_with_embedded_nul_text() {
    // Arrange
    use_local_storage();
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
