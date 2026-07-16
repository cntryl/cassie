use std::future::{ready, Ready};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use serde_json::json;

use super::context::{
    benchmark_data_dir_for_mode, configure_benchmark_environment, prepare_collection, BenchContext,
    BenchIndexOptions, BenchmarkStorageMode, ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};

const JOIN_USERS: &str = "bench_join_users";
const JOIN_ORDERS: &str = "bench_join_orders";
const GRAPH: &str = "bench_graph";
const TIME_SERIES: &str = "bench_time_series_events";

pub fn tier3_query_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(tier3_query_context_now(label, dataset_rows))
}

fn tier3_query_context_now(label: &str, dataset_rows: usize) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let data_dir = benchmark_data_dir_for_mode(label, BenchmarkStorageMode::Disk);
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 1_024;
    config.limits.operator_switch_join_row_threshold = dataset_rows.saturating_mul(2).max(1);

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(
        data_dir.clone(),
        config,
    )?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let context = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir,
        _embedding_server: None,
    };
    prepare_collection(&context, dataset_rows, BenchIndexOptions::full())?;
    Ok(context)
}

#[derive(Debug, Clone, Copy)]
pub struct Tier3QueryDomains {
    pub join: bool,
    pub graph: bool,
    pub time_series: bool,
}

pub fn prepare_tier3_query_domains(
    context: &BenchContext,
    dataset_rows: usize,
    domains: Tier3QueryDomains,
) -> Result<(), CassieError> {
    if domains.join {
        prepare_join_collections(context, dataset_rows)?;
    }
    if domains.graph {
        prepare_graph(context, dataset_rows)?;
    }
    if domains.time_series {
        prepare_time_series(context, dataset_rows)?;
    }
    Ok(())
}

fn prepare_join_collections(
    context: &BenchContext,
    dataset_rows: usize,
) -> Result<(), CassieError> {
    if context.cassie.catalog.exists(JOIN_USERS) {
        return Ok(());
    }

    execute_ddl(
        context,
        "CREATE TABLE bench_join_users (user_key INT, name TEXT)",
    )?;
    execute_ddl(
        context,
        "CREATE TABLE bench_join_orders (order_user_key INT, total INT)",
    )?;

    let users = (0..dataset_rows)
        .map(|index| {
            let key = index_as_i64(index);
            (
                Some(format!("user-{index}")),
                json!({
                    "user_key": key,
                    "name": format!("user-{index}"),
                }),
            )
        })
        .collect::<Vec<_>>();
    let orders = (0..dataset_rows)
        .map(|index| {
            (
                Some(format!("order-{index}")),
                json!({
                    "order_user_key": index_as_i64(index),
                    "total": index_as_i64(index % 100),
                }),
            )
        })
        .collect::<Vec<_>>();
    context
        .cassie
        .midge
        .put_fresh_documents(JOIN_USERS, users)?;
    context
        .cassie
        .midge
        .put_fresh_documents(JOIN_ORDERS, orders)?;
    Ok(())
}

fn prepare_graph(context: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
    if context.cassie.catalog.graph_exists(GRAPH) {
        return Ok(());
    }

    execute_ddl(
        context,
        "CREATE GRAPH bench_graph (NODES (label TEXT), EDGES (source TEXT))",
    )?;
    let nodes = (0..dataset_rows)
        .map(|index| {
            (
                Some(format!("node-{index}")),
                json!({
                    "node_type": "doc",
                    "node_id": format!("node-{index}"),
                    "label": format!("Node {index}"),
                }),
            )
        })
        .collect::<Vec<_>>();
    context
        .cassie
        .midge
        .put_fresh_graph_documents("bench_graph_nodes", nodes)?;

    let edges = (0..dataset_rows.saturating_sub(1))
        .map(|index| {
            (
                Some(format!("edge-{index}")),
                json!({
                    "edge_id": format!("edge-{index}"),
                    "source_type": "doc",
                    "source_id": format!("node-{index}"),
                    "target_type": "doc",
                    "target_id": format!("node-{}", index + 1),
                    "edge_type": "links",
                    "weight": 1,
                    "source": "bench",
                }),
            )
        })
        .collect::<Vec<_>>();
    context
        .cassie
        .midge
        .put_fresh_graph_documents("bench_graph_edges", edges)?;
    Ok(())
}

fn prepare_time_series(context: &BenchContext, dataset_rows: usize) -> Result<(), CassieError> {
    if context.cassie.catalog.exists(TIME_SERIES) {
        return Ok(());
    }

    execute_ddl(
        context,
        "CREATE TABLE bench_time_series_events (tenant TEXT, event_at TIMESTAMP, amount INT, status TEXT)",
    )?;
    for statement in [
        "CREATE INDEX bench_time_series_time_idx ON bench_time_series_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
        "CREATE ROLLUP bench_time_series_hourly ON bench_time_series_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
        "CREATE RETENTION POLICY bench_time_series_retention ON bench_time_series_events USING event_at RETAIN FOR '2 days'",
    ] {
        execute_ddl(context, statement)?;
    }

    let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d"];
    let documents = (0..dataset_rows)
        .map(|index| {
            let day = 9 + ((index / 24) % 7);
            let hour = index % 24;
            (
                Some(format!("ts-doc-{index}")),
                json!({
                    "tenant": tenants[index % tenants.len()],
                    "event_at": format!("2026-01-{day:02}T{hour:02}:00:00Z"),
                    "amount": index_as_i64(index % 100),
                    "status": if index % 2 == 0 { "open" } else { "closed" },
                }),
            )
        })
        .collect::<Vec<_>>();
    context
        .cassie
        .midge
        .put_fresh_time_series_documents(TIME_SERIES, documents)?;
    Ok(())
}

fn execute_ddl(context: &BenchContext, sql: &str) -> Result<(), CassieError> {
    context
        .cassie
        .execute_sql(&context.session, sql, Vec::new())
        .map(|_| ())
}

fn index_as_i64(index: usize) -> i64 {
    i64::try_from(index).expect("Tier 3 fixture row index should fit i64")
}
