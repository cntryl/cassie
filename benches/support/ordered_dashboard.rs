use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

use crate::performance_benchmarks::FixtureClass;
use crate::stress::{CassieStressRunner, FixtureDeclaration, OperationUnit, StressCase};

const BUDGET: usize = 512 * 1024 * 1024;
const QUERY: &str = "SELECT dashboard_orders.id,dashboard_orders.payload,     dashboard_accounts.name,dashboard_products.label,dashboard_products.price FROM dashboard_orders     JOIN dashboard_accounts ON dashboard_orders.account_id=dashboard_accounts.id     JOIN dashboard_products ON dashboard_orders.product_id=dashboard_products.id     WHERE dashboard_orders.tenant_id=7 AND dashboard_orders.status='open'     ORDER BY dashboard_orders.created_at DESC,dashboard_orders.id DESC LIMIT 10";
const CREATE_INDEX: &str = "CREATE INDEX dashboard_order_feed ON dashboard_orders     (tenant_id,created_at DESC,id DESC) INCLUDE (payload,account_id,product_id) WHERE status='open'";

pub(crate) fn run(runner: &mut CassieStressRunner) {
    for (scale, noise) in [("1k", 1_000), ("10k", 10_000)] {
        for (workload, indexed) in [
            ("ordered_dashboard_indexed", true),
            ("ordered_dashboard_scan", false),
        ] {
            let case = StressCase::new(workload, scale).runtime_contract(
                FixtureDeclaration::new(
                    FixtureClass::Representative,
                    noise,
                    format!("tier3_system_query/dashboard/{scale}"),
                ),
                OperationUnit::Query,
            );
            if runner.is_enabled(&case) {
                measure(runner, case, noise, indexed);
            }
        }
    }
}

fn measure(runner: &mut CassieStressRunner, case: StressCase, noise: usize, indexed: bool) {
    let setup = Instant::now();
    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    let mut config = CassieRuntimeConfig::from_env().expect("dashboard benchmark config");
    config.limits.query_memory_budget_bytes = BUDGET;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(config).expect("dashboard memory store"));
    cassie.startup().expect("dashboard startup");
    let session = cassie.create_session("benchmark", None);
    seed(&cassie, &session, noise, indexed);
    let setup_ns = setup.elapsed().as_nanos().to_string();

    let result = cassie
        .execute_sql(&session, QUERY, Vec::new())
        .expect("dashboard preflight");
    let expected = (3_i64..=12)
        .rev()
        .map(|id| {
            vec![
                Value::Int64(id),
                Value::String(format!("target-{id}")),
                Value::String(format!("account-{id}")),
                Value::String(format!("product-{}", id + 10)),
                Value::Int64((id + 10) * 7),
            ]
        })
        .collect::<Vec<_>>();
    assert_eq!(result.rows(), expected);
    let plan = cassie
        .execute_sql(&session, &format!("EXPLAIN ANALYZE {QUERY}"), Vec::new())
        .expect("dashboard preflight plan");
    let plan = format!("{:?}", plan.rows());
    assert_eq!(
        plan.contains("ordered_index_nested_loop"),
        indexed,
        "{plan}"
    );
    let path = if indexed {
        "ordered_index_nested_loop"
    } else {
        "unindexed_join"
    };
    let fallback = if indexed { "none" } else { "index_absent" };
    let case = case
        .metadata("setup_time_ns", setup_ns)
        .metadata("query_memory_budget_bytes", BUDGET.to_string())
        .metadata("unrelated_orders", noise.to_string())
        .metadata("storage_mode", "memory")
        .preflight_evidence(path, fallback)
        .runtime_evidence(Arc::clone(&cassie));
    runner.measure_batch(case, 1, || {
        let result = cassie
            .execute_sql(&session, QUERY, Vec::new())
            .expect("dashboard query");
        assert_eq!(result.rows().len(), 10);
        std::hint::black_box(result.rows().len())
    });
    cassie.shutdown();
}

fn seed(cassie: &Cassie, session: &CassieSession, noise: usize, indexed: bool) {
    for statement in [
        "CREATE TABLE dashboard_orders (id INTEGER PRIMARY KEY, tenant_id INTEGER NOT NULL, status TEXT NOT NULL, created_at TIMESTAMP NOT NULL, payload TEXT NOT NULL, account_id INTEGER NOT NULL, product_id INTEGER NOT NULL)",
        "CREATE TABLE dashboard_accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE dashboard_products (id INTEGER PRIMARY KEY, label TEXT NOT NULL, price INTEGER NOT NULL)",
    ] {
        execute(cassie, session, statement);
    }
    if indexed {
        execute(cassie, session, CREATE_INDEX);
    }
    let accounts: Vec<_> = (1..=16)
        .map(|id| format!("({id},'account-{id}')"))
        .collect();
    let products: Vec<_> = (1..=32)
        .map(|id| format!("({id},'product-{id}',{})", id * 7))
        .collect();
    execute(
        cassie,
        session,
        &format!(
            "INSERT INTO dashboard_accounts VALUES {}",
            accounts.join(",")
        ),
    );
    execute(
        cassie,
        session,
        &format!(
            "INSERT INTO dashboard_products VALUES {}",
            products.join(",")
        ),
    );
    let target: Vec<_> = (1..=12)
        .map(|id| {
            format!(
                "({id},7,'open',TIMESTAMP '2026-01-01 00:00:{:02}','target-{id}',{id},{})",
                (id - 1) / 2,
                id + 10
            )
        })
        .collect();
    execute(
        cassie,
        session,
        &format!("INSERT INTO dashboard_orders VALUES {}", target.join(",")),
    );
    for start in (0..noise).step_by(256) {
        let mut sql = String::from("INSERT INTO dashboard_orders VALUES ");
        for offset in start..(start + 256).min(noise) {
            if offset != start {
                sql.push(',');
            }
            let (tenant, status, minute) = if offset % 5 == 0 {
                (99, "open", 2)
            } else {
                (7, "closed", 1)
            };
            write!(sql, "({}, {tenant},'{status}',TIMESTAMP '2026-01-01 00:{minute:02}:{:02}','noise-{offset}',{},{})",
                offset + 100_000, offset % 4, offset % 16 + 1, offset % 32 + 1).expect("write dashboard fixture");
        }
        execute(cassie, session, &sql);
    }
}

fn execute(cassie: &Cassie, session: &CassieSession, statement: &str) {
    cassie
        .execute_sql(session, statement, Vec::new())
        .expect("dashboard fixture statement");
}
