use std::future::{ready, Ready};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::catalog::CollectionCardinalityStats;
use cassie::config::CassieRuntimeConfig;
use serde_json::json;

use super::context::{
    benchmark_data_dir, configure_benchmark_environment, usize_mod_i64, usize_to_i64, BenchContext,
    ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};
use super::document_batches::bench_document_write_batch_ranges;

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

pub fn vectorized_right_indexed_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_right_indexed_join_context_now(
        label,
        dataset_rows,
    ))
}

fn vectorized_right_indexed_join_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    let ctx = vectorized_join_context_now(label, dataset_rows)?;
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_join_orders_key_idx ON bench_join_orders USING btree (order_user_key)",
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

pub fn vectorized_late_match_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_late_match_join_context_now(label, dataset_rows))
}

pub fn vectorized_fanout_join_context(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(vectorized_fanout_join_context_now(label, dataset_rows))
}

fn vectorized_late_match_join_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    let ctx = vectorized_join_context_with_budget(
        label,
        dataset_rows,
        JoinLoadShape::LateMatchRight {
            user_rows: 50,
            order_rows: dataset_rows,
        },
        None,
    )?;
    Ok(ctx)
}

fn vectorized_fanout_join_context_now(
    label: &str,
    dataset_rows: usize,
) -> Result<BenchContext, CassieError> {
    let user_rows = dataset_rows / 3;
    let ctx = vectorized_join_context_with_budget(
        label,
        dataset_rows,
        JoinLoadShape::FanoutRight {
            user_rows,
            order_rows: dataset_rows,
            key_count: 10,
        },
        None,
    )?;
    hydrate_join_row_count(&ctx, "bench_join_users", user_rows);
    hydrate_join_row_count(&ctx, "bench_join_orders", dataset_rows);
    Ok(ctx)
}

fn vectorized_join_context_with_budget(
    label: &str,
    dataset_rows: usize,
    shape: JoinLoadShape,
    temp_budget_bytes: Option<usize>,
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let dir = benchmark_data_dir(label);
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 1024;
    config.limits.operator_switch_join_row_threshold = dataset_rows.saturating_mul(2).max(1);
    config.limits.query_memory_budget_bytes =
        temp_budget_bytes.unwrap_or(ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES);

    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir.clone(), config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_join_users".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_vectorized_join_collections(&ctx, dataset_rows, shape)?;
    Ok(ctx)
}

#[derive(Debug, Clone, Copy)]
enum JoinLoadShape {
    OneToOne {
        order_rows: usize,
    },
    DenseRight {
        order_rows: usize,
    },
    LateMatchRight {
        user_rows: usize,
        order_rows: usize,
    },
    FanoutRight {
        user_rows: usize,
        order_rows: usize,
        key_count: usize,
    },
}

type JoinDocuments = Vec<(Option<String>, serde_json::Value)>;

impl JoinLoadShape {
    fn user_rows(self, dataset_rows: usize) -> usize {
        match self {
            Self::OneToOne { .. } | Self::DenseRight { .. } => dataset_rows,
            Self::LateMatchRight { user_rows, .. } | Self::FanoutRight { user_rows, .. } => {
                user_rows
            }
        }
    }

    fn order_rows(self) -> usize {
        match self {
            Self::OneToOne { order_rows }
            | Self::DenseRight { order_rows }
            | Self::LateMatchRight { order_rows, .. }
            | Self::FanoutRight { order_rows, .. } => order_rows,
        }
    }

    fn user_key(self, index: usize) -> i64 {
        match self {
            Self::FanoutRight { key_count, .. } => usize_mod_i64(index, key_count),
            Self::OneToOne { .. } | Self::DenseRight { .. } | Self::LateMatchRight { .. } => {
                usize_to_i64(index)
            }
        }
    }

    fn order_user_key(self, index: usize, order_rows: usize) -> i64 {
        match self {
            Self::OneToOne { .. } => usize_to_i64(index),
            Self::DenseRight { .. } => 0_i64,
            Self::LateMatchRight { user_rows, .. } => {
                late_match_order_user_key(index, order_rows, user_rows)
            }
            Self::FanoutRight { key_count, .. } => usize_mod_i64(index, key_count),
        }
    }
}

fn prepare_vectorized_join_collections(
    ctx: &BenchContext,
    dataset_rows: usize,
    shape: JoinLoadShape,
) -> Result<(), CassieError> {
    if ctx.cassie.catalog.exists("bench_join_users") {
        return Ok(());
    }

    ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE TABLE bench_join_users (user_key INT, name TEXT)",
        vec![],
    )?;
    ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE TABLE bench_join_orders (order_user_key INT, total INT)",
        vec![],
    )?;

    put_join_documents_in_batches(
        ctx,
        "bench_join_users",
        shape.user_rows(dataset_rows),
        |range| join_user_documents(shape, range),
    )?;
    put_join_documents_in_batches(ctx, "bench_join_orders", shape.order_rows(), |range| {
        join_order_documents(shape, range)
    })?;
    hydrate_join_row_count(ctx, "bench_join_users", shape.user_rows(dataset_rows));
    hydrate_join_row_count(ctx, "bench_join_orders", shape.order_rows());

    Ok(())
}

pub(super) fn prepare_scaling_join_collections(
    ctx: &BenchContext,
    dataset_rows: usize,
) -> Result<(), CassieError> {
    prepare_vectorized_join_collections(
        ctx,
        dataset_rows,
        JoinLoadShape::OneToOne {
            order_rows: dataset_rows,
        },
    )
}

pub fn activate_scaling_join_curve_index(ctx: &BenchContext) -> Result<(), CassieError> {
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_join_users_key_idx ON bench_join_users USING btree (user_key)",
        vec![],
    )?;
    Ok(())
}

pub fn deactivate_scaling_join_curve_index(ctx: &BenchContext) -> Result<(), CassieError> {
    ctx.cassie
        .execute_sql(
            &ctx.session,
            "DROP INDEX bench_join_users_key_idx ON bench_join_users",
            vec![],
        )
        .map(|_| ())
}

pub fn prepare_legacy_scaling_join_collection(
    ctx: &BenchContext,
    dataset_rows: usize,
    workload: &str,
) -> Result<(), CassieError> {
    match workload {
        "vectorized_left_join_limited"
        | "vectorized_indexed_inner_join"
        | "vectorized_right_indexed_inner_join" => Ok(()),
        "vectorized_streaming_inner_join" => prepare_named_join_collections(
            ctx,
            dataset_rows,
            JoinLoadShape::OneToOne { order_rows: 50 },
            LegacyJoinVariant::Sparse,
        ),
        "vectorized_dense_streaming_inner_join" => prepare_named_join_collections(
            ctx,
            dataset_rows,
            JoinLoadShape::DenseRight {
                order_rows: dataset_rows,
            },
            LegacyJoinVariant::Dense,
        ),
        "vectorized_late_match_inner_join" => prepare_named_join_collections(
            ctx,
            dataset_rows,
            JoinLoadShape::LateMatchRight {
                user_rows: 50,
                order_rows: dataset_rows,
            },
            LegacyJoinVariant::LateMatch,
        ),
        "vectorized_fanout_inner_join" => prepare_named_join_collections(
            ctx,
            dataset_rows,
            JoinLoadShape::FanoutRight {
                user_rows: dataset_rows / 3,
                order_rows: dataset_rows,
                key_count: 10,
            },
            LegacyJoinVariant::Fanout,
        ),
        other => Err(CassieError::Execution(format!(
            "unsupported legacy join scaling workload '{other}'"
        ))),
    }
}

pub fn activate_legacy_join_variant(ctx: &BenchContext, workload: &str) -> Result<(), CassieError> {
    let statement = match workload {
        "vectorized_indexed_inner_join" => Some(
            "CREATE INDEX bench_join_users_key_idx ON bench_join_users USING btree (user_key)",
        ),
        "vectorized_right_indexed_inner_join" => Some(
            "CREATE INDEX bench_join_orders_key_idx ON bench_join_orders USING btree (order_user_key)",
        ),
        _ => None,
    };
    statement.map_or(Ok(()), |sql| {
        ctx.cassie
            .execute_sql(&ctx.session, sql, vec![])
            .map(|_| ())
    })
}

pub fn deactivate_legacy_join_variant(
    ctx: &BenchContext,
    workload: &str,
) -> Result<(), CassieError> {
    let statement = match workload {
        "vectorized_indexed_inner_join" => {
            Some("DROP INDEX bench_join_users_key_idx ON bench_join_users")
        }
        "vectorized_right_indexed_inner_join" => {
            Some("DROP INDEX bench_join_orders_key_idx ON bench_join_orders")
        }
        _ => None,
    };
    statement.map_or(Ok(()), |sql| {
        ctx.cassie
            .execute_sql(&ctx.session, sql, vec![])
            .map(|_| ())
    })
}

#[derive(Debug, Clone, Copy)]
enum LegacyJoinVariant {
    Sparse,
    Dense,
    LateMatch,
    Fanout,
}

impl LegacyJoinVariant {
    const fn collections(self) -> (&'static str, &'static str) {
        match self {
            Self::Sparse => ("bench_sparse_users", "bench_sparse_orders"),
            Self::Dense => ("bench_dense_users", "bench_dense_orders"),
            Self::LateMatch => ("bench_late_users", "bench_late_orders"),
            Self::Fanout => ("bench_fanout_users", "bench_fanout_orders"),
        }
    }
}

fn prepare_named_join_collections(
    ctx: &BenchContext,
    dataset_rows: usize,
    shape: JoinLoadShape,
    variant: LegacyJoinVariant,
) -> Result<(), CassieError> {
    let (users_collection, orders_collection) = variant.collections();
    if ctx.cassie.catalog.exists(users_collection) {
        return Ok(());
    }
    ctx.cassie.execute_sql(
        &ctx.session,
        &format!("CREATE TABLE {users_collection} (user_key INT, name TEXT)"),
        vec![],
    )?;
    ctx.cassie.execute_sql(
        &ctx.session,
        &format!("CREATE TABLE {orders_collection} (order_user_key INT, total INT)"),
        vec![],
    )?;
    put_join_documents_in_batches(
        ctx,
        users_collection,
        shape.user_rows(dataset_rows),
        |range| join_user_documents(shape, range),
    )?;
    put_join_documents_in_batches(ctx, orders_collection, shape.order_rows(), |range| {
        join_order_documents(shape, range)
    })?;
    hydrate_join_row_count(ctx, users_collection, shape.user_rows(dataset_rows));
    hydrate_join_row_count(ctx, orders_collection, shape.order_rows());
    Ok(())
}

fn put_join_documents_in_batches(
    ctx: &BenchContext,
    collection: &str,
    row_count: usize,
    build: impl Fn(std::ops::Range<usize>) -> JoinDocuments,
) -> Result<(), CassieError> {
    for range in bench_document_write_batch_ranges(row_count) {
        let start = range.start;
        let end = range.end;
        let documents = build(range);
        ctx.cassie
            .midge
            .put_fresh_documents(collection, documents)
            .map_err(|error| {
                CassieError::Execution(format!(
                    "seed scaling join relation {collection} rows {start}..{end}: {error}"
                ))
            })?;
    }
    Ok(())
}

fn join_user_documents(shape: JoinLoadShape, range: std::ops::Range<usize>) -> JoinDocuments {
    let mut users = Vec::with_capacity(range.len());
    for index in range {
        users.push((
            Some(format!("user-{index}")),
            json!({
                "user_key": shape.user_key(index),
                "name": format!("user-{index}"),
            }),
        ));
    }
    users
}

fn join_order_documents(shape: JoinLoadShape, range: std::ops::Range<usize>) -> JoinDocuments {
    let order_rows = shape.order_rows();
    let mut orders = Vec::with_capacity(range.len());
    for index in range {
        orders.push((
            Some(format!("order-{index}")),
            json!({
                "order_user_key": shape.order_user_key(index, order_rows),
                "total": usize_mod_i64(index, 100),
            }),
        ));
    }
    orders
}

fn late_match_order_user_key(index: usize, order_rows: usize, user_rows: usize) -> i64 {
    let first_match = order_rows.saturating_sub(user_rows);
    if index >= first_match {
        usize_to_i64(index - first_match)
    } else {
        usize_to_i64(order_rows.saturating_add(index))
    }
}

fn hydrate_join_row_count(ctx: &BenchContext, collection: &str, row_count: usize) {
    ctx.cassie.catalog.hydrate_cardinality_stats(
        collection,
        CollectionCardinalityStats {
            row_count: u64::try_from(row_count).unwrap_or(u64::MAX),
            ..CollectionCardinalityStats::default()
        },
    );
}
