use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};

pub fn circle_fixture(name: &str, rows: u32, budget: usize) -> Cassie {
    circle_fixture_with_row_limit(name, rows, budget, 100_000)
}

pub fn circle_fixture_with_row_limit(
    _name: &str,
    rows: u32,
    budget: usize,
    max_result_rows: usize,
) -> Cassie {
    let cassie = new_circle_fixture(budget, max_result_rows);
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "CREATE TABLE docs (content TEXT, embedding VECTOR(3))",
            vec![],
        )
        .expect("create vector table");
    let documents = (0..rows)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / f64::from(rows);
            (
                Some(format!("row-{index:02}")),
                serde_json::json!({
                    "content": "query", "embedding": [angle.cos(), angle.sin(), 1.0]
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("docs", documents)
        .expect("seed vectors");
    cassie
}

fn new_circle_fixture(budget: usize, max_result_rows: usize) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "retrieval-controls".to_string(),
        dimensions: 3,
    });
    config.limits.query_memory_budget_bytes = budget;
    config.limits.max_result_rows = max_result_rows;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    Cassie::new_in_memory_with_config(config).expect("create in-memory Cassie")
}

pub fn create_index(cassie: &Cassie, options: &str) {
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            &format!(
                "CREATE INDEX docs_vector ON docs USING vector (embedding) WITH (source_field = content, metric = l2, {options})"
            ),
            vec![],
        )
        .expect("create vector index");
}
