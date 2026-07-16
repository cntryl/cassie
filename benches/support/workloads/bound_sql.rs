use cassie::types::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct BoundBenchmarkSql {
    pub sql: String,
    pub params: Vec<Value>,
}

#[must_use]
pub fn plan_cache_miss(nonce: usize) -> BoundBenchmarkSql {
    let alias = plan_miss_alias(nonce);
    BoundBenchmarkSql {
        sql: format!(
            "SELECT id AS {alias}, title FROM bench_documents WHERE score >= $1 AND status IN ($2, $3, $4) LIMIT 20"
        ),
        params: vec![
            Value::Int64(10),
            Value::String("approved".to_string()),
            Value::String("pending".to_string()),
            Value::String(format!("miss-{nonce}")),
        ],
    }
}

#[must_use]
pub fn recursive_cte(upper_bound: usize) -> BoundBenchmarkSql {
    BoundBenchmarkSql {
        sql: "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT CAST(seq.n + 1 AS INT) FROM seq JOIN recursive_cte_fanout ON recursive_cte_fanout.n = 1 WHERE seq.n < $1) SELECT n FROM seq".to_string(),
        params: vec![Value::Int64(
            i64::try_from(upper_bound).expect("recursive bound should fit i64"),
        )],
    }
}

#[must_use]
pub fn time_series_window(start: &str, end: &str) -> BoundBenchmarkSql {
    BoundBenchmarkSql {
        sql: "SELECT tenant, amount FROM bench_documents WHERE event_at >= $1 AND event_at < $2 ORDER BY event_at LIMIT 512".to_string(),
        params: vec![
            Value::String(start.to_string()),
            Value::String(end.to_string()),
        ],
    }
}

fn plan_miss_alias(nonce: usize) -> &'static str {
    const ALIASES: [&str; 64] = [
        "miss_00", "miss_01", "miss_02", "miss_03", "miss_04", "miss_05", "miss_06", "miss_07",
        "miss_08", "miss_09", "miss_10", "miss_11", "miss_12", "miss_13", "miss_14", "miss_15",
        "miss_16", "miss_17", "miss_18", "miss_19", "miss_20", "miss_21", "miss_22", "miss_23",
        "miss_24", "miss_25", "miss_26", "miss_27", "miss_28", "miss_29", "miss_30", "miss_31",
        "miss_32", "miss_33", "miss_34", "miss_35", "miss_36", "miss_37", "miss_38", "miss_39",
        "miss_40", "miss_41", "miss_42", "miss_43", "miss_44", "miss_45", "miss_46", "miss_47",
        "miss_48", "miss_49", "miss_50", "miss_51", "miss_52", "miss_53", "miss_54", "miss_55",
        "miss_56", "miss_57", "miss_58", "miss_59", "miss_60", "miss_61", "miss_62", "miss_63",
    ];
    ALIASES[nonce % ALIASES.len()]
}
