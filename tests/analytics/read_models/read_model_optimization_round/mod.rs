#![allow(unused_imports, dead_code)]

use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use tokio_postgres::NoTls;

use super::support_sql as support;
use support::*;

fn vectorized_join_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config.limits.adaptive_execution_enabled = false;
    config.limits.operator_switching_enabled = cassie::config::OperatorSwitchingEnabled::disabled();
    config
}

fn canonical_collection(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
}

fn seed_read_model_hot_paths(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE read_model_hot_paths \
             (tenant TEXT, status TEXT, created_at INT, title TEXT, body TEXT)",
            vec![],
        )
        .unwrap();
    for (id, tenant, status, created_at, title, body) in [
        ("row-1", "tenant-a", "open", 30, "Gamma", "third"),
        ("row-2", "tenant-a", "open", 10, "Alpha", "first"),
        ("row-3", "tenant-a", "open", 20, "Beta", "second"),
        ("row-4", "tenant-a", "closed", 5, "Closed", "closed"),
        ("row-5", "tenant-b", "open", 1, "Other", "other"),
    ] {
        cassie
            .midge
            .put_document(
                "read_model_hot_paths",
                Some(id.to_string()),
                serde_json::json!({
                    "tenant": tenant,
                    "status": status,
                    "created_at": created_at,
                    "title": title,
                    "body": body
                }),
            )
            .unwrap();
    }
    for sql in [
        "CREATE INDEX read_model_hot_paths_tenant_status_time_idx \
         ON read_model_hot_paths USING btree (tenant, status, created_at)",
        "CREATE INDEX read_model_hot_paths_lower_title_idx \
         ON read_model_hot_paths USING btree (lower(title))",
    ] {
        cassie.execute_sql(session, sql, vec![]).unwrap();
    }
}

fn assert_hot_path_results(
    page: &cassie::executor::QueryResult,
    expression: &cassie::executor::QueryResult,
) {
    assert_eq!(
        page.rows(),
        vec![
            vec![Value::String("first".to_string())],
            vec![Value::String("second".to_string())],
        ]
    );
    assert_eq!(
        expression.rows(),
        vec![vec![Value::String("first".to_string())]]
    );
}

fn assert_hot_path_plans(
    page_explain: &cassie::executor::QueryResult,
    expression_explain: &cassie::executor::QueryResult,
) {
    let page_plan = explain_plan_text(page_explain);
    assert_explain_contains(
        page_plan,
        "index",
        "read_model_hot_paths_tenant_status_time_idx",
    );
    assert_explain_contains(page_plan, "access_path", "range_scan");
    assert_explain_contains(page_plan, "fallback_reason", "none");

    let expression_plan = explain_plan_text(expression_explain);
    assert_explain_contains(
        expression_plan,
        "index",
        "read_model_hot_paths_lower_title_idx",
    );
    assert_explain_contains(expression_plan, "access_path", "index_seek");
    assert_explain_contains(expression_plan, "fallback_reason", "none");
}

fn assert_hot_path_metrics(before: &serde_json::Value, after: &serde_json::Value) {
    assert!(
        after["read_paths"]["range_scans"].as_u64().unwrap()
            > before["read_paths"]["range_scans"].as_u64().unwrap()
    );
    assert!(
        after["read_paths"]["index_seek_scans"].as_u64().unwrap()
            > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
    );
}

fn seed_indexed_join_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE read_model_indexed_users (user_key INT, name TEXT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE TABLE read_model_indexed_orders (order_user_key INT, total INT)",
            vec![],
        )
        .unwrap();

    let users = (0..200)
        .map(|index| {
            (
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            )
        })
        .collect();
    let users_collection = canonical_collection("read_model_indexed_users");
    cassie
        .midge
        .put_fresh_documents(&users_collection, users)
        .unwrap();
    let orders_collection = canonical_collection("read_model_indexed_orders");
    cassie
        .midge
        .put_fresh_documents(
            &orders_collection,
            vec![
                (
                    Some("order-150".to_string()),
                    serde_json::json!({"order_user_key": 150_i64, "total": 150_i64}),
                ),
                (
                    Some("order-151".to_string()),
                    serde_json::json!({"order_user_key": 151_i64, "total": 151_i64}),
                ),
            ],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE INDEX read_model_indexed_users_key_idx \
             ON read_model_indexed_users USING btree (user_key)",
            vec![],
        )
        .unwrap();
}

fn assert_indexed_join_result(
    result: &cassie::executor::QueryResult,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    assert_eq!(
        result.rows(),
        vec![
            vec![Value::String("user-150".to_string()), Value::Int64(150)],
            vec![Value::String("user-151".to_string()), Value::Int64(151)],
        ]
    );
    assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
    let probe_delta = after["joins"]["vectorized_probe_rows_total"]
        .as_u64()
        .unwrap()
        - before["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap();
    let index_seek_delta = after["read_paths"]["index_seek_scans"].as_u64().unwrap()
        - before["read_paths"]["index_seek_scans"].as_u64().unwrap();
    assert!(
        probe_delta <= 2,
        "expected indexed bounded join to probe only matching left rows, got {probe_delta}"
    );
    assert!(
        index_seek_delta > 0,
        "expected bounded inner join to use the indexed left source"
    );
}

include!("semantics.rs");

include!("resources.rs");
