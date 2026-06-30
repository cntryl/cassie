use std::future::{ready, Ready};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use serde_json::json;

use super::context::{benchmark_data_dir, usize_mod_i64, usize_to_i64, BenchContext};

pub fn vectorized_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_join_context_now(label, dataset_rows))
}

fn vectorized_join_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    let ctx = vectorized_join_context_with_budget(
        label,
        dataset_rows,
        JoinLoadShape::OneToOne {
            order_rows: dataset_rows,
        },
        None,
    )?;
    Ok(ctx)
}

pub fn vectorized_indexed_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_indexed_join_context_now(label, dataset_rows))
}

fn vectorized_indexed_join_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    let ctx = vectorized_join_context_now(label, dataset_rows)?;
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_join_users_key_idx ON bench_join_users USING btree (user_key)",
        vec![],
    )?;
    Ok(ctx)
}

pub fn vectorized_sparse_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_join_context_with_budget(
        label,
        dataset_rows,
        JoinLoadShape::OneToOne { order_rows: 50 },
        None,
    ))
}

pub fn vectorized_dense_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_join_context_with_budget(
        label,
        dataset_rows,
        JoinLoadShape::DenseRight {
            order_rows: dataset_rows,
        },
        Some(4 * 1024),
    ))
}

fn vectorized_join_context_with_budget(
    label: &str,
    dataset_rows: usize,
    shape: JoinLoadShape,
    temp_budget_bytes: Option<usize>,
) -> Result<BenchContext, CassieError> {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let dir = benchmark_data_dir(label);
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 1024;
    config.limits.operator_switch_join_row_threshold = dataset_rows.saturating_mul(2).max(1);
    config.limits.temp_spill_budget_bytes = temp_budget_bytes.unwrap_or_else(|| {
        config
            .limits
            .temp_spill_budget_bytes
            .max(dataset_rows.saturating_mul(1024))
    });

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir, config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_join_users".to_string(),
        _embedding_server: None,
    };
    prepare_vectorized_join_collections(&ctx, dataset_rows, shape)?;
    Ok(ctx)
}

#[derive(Debug, Clone, Copy)]
enum JoinLoadShape {
    OneToOne { order_rows: usize },
    DenseRight { order_rows: usize },
}

fn prepare_vectorized_join_collections(
    ctx: &BenchContext,
    dataset_rows: usize,
    shape: JoinLoadShape,
) -> Result<(), CassieError> {
    if ctx.cassie.catalog.exists("bench_join_users") {
        return Ok(());
    }

    let user_schema = Schema {
        fields: vec![
            FieldSchema {
                name: "user_key".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };
    let order_schema = Schema {
        fields: vec![
            FieldSchema {
                name: "order_user_key".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "total".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    };
    ctx.cassie
        .midge
        .create_collection("bench_join_users", user_schema.clone())?;
    ctx.cassie
        .midge
        .create_collection("bench_join_orders", order_schema.clone())?;
    ctx.cassie.register_collection(
        "bench_join_users",
        user_schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    ctx.cassie.register_collection(
        "bench_join_orders",
        order_schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );

    let mut users = Vec::with_capacity(dataset_rows);
    for index in 0..dataset_rows {
        users.push((
            Some(format!("user-{index}")),
            json!({
                "user_key": usize_to_i64(index),
                "name": format!("user-{index}"),
            }),
        ));
    }

    let order_rows = match shape {
        JoinLoadShape::OneToOne { order_rows } | JoinLoadShape::DenseRight { order_rows } => {
            order_rows
        }
    };
    let mut orders = Vec::with_capacity(order_rows);
    for index in 0..order_rows {
        let order_user_key = match shape {
            JoinLoadShape::OneToOne { .. } => usize_to_i64(index),
            JoinLoadShape::DenseRight { .. } => 0_i64,
        };
        orders.push((
            Some(format!("order-{index}")),
            json!({
                "order_user_key": order_user_key,
                "total": usize_mod_i64(index, 100),
            }),
        ));
    }

    if !users.is_empty() {
        ctx.cassie
            .midge
            .put_fresh_documents("bench_join_users", users)?;
    }
    if !orders.is_empty() {
        ctx.cassie
            .midge
            .put_fresh_documents("bench_join_orders", orders)?;
    }

    Ok(())
}
