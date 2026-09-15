use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, OperatorSwitchingEnabled};
use cassie::runtime::{RuntimeFeedbackKey, RuntimeFeedbackObservation};
use cassie::types::Value;
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const TABLE: &str = "adaptive_profile_evidence";
const BASE_INDEX: &str = "adaptive_profile_body_idx";
const PREFERRED_INDEX: &str = "adaptive_profile_title_idx";
const SELECT_SQL: &str = "SELECT title, body, sequence FROM adaptive_profile_evidence WHERE title = 'alpha' AND body = 'one' ORDER BY title, body, sequence";

pub struct AdaptiveProfileEvidence {
    fixed: ProfileObservation,
    adaptive: ProfileObservation,
}

struct ProfileObservation {
    profile: &'static str,
    rows: Vec<Vec<Value>>,
    first_page: Vec<Vec<Value>>,
    second_page: Vec<Vec<Value>>,
    transaction_visible: Vec<Vec<Value>>,
    transaction_hidden: Vec<Vec<Value>>,
    join_rows: Vec<Vec<Value>>,
    error: String,
    selected_plan: String,
    pagination_plan: String,
    join_plan: String,
    fallback_plan: String,
    metrics: JsonValue,
    config: JsonValue,
}

struct SemanticObservations {
    rows: Vec<Vec<Value>>,
    first_page: Vec<Vec<Value>>,
    second_page: Vec<Vec<Value>>,
    transaction_visible: Vec<Vec<Value>>,
    transaction_hidden: Vec<Vec<Value>>,
    join_rows: Vec<Vec<Value>>,
    error: String,
}

impl AdaptiveProfileEvidence {
    pub fn collect() -> Self {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("adaptive evidence runtime");
        runtime.block_on(async {
            Self {
                fixed: observe_profile("disabled-default", disabled_config(), false),
                adaptive: observe_profile("evaluation-conservative", evaluation_config(), true),
            }
        })
    }

    pub fn assert_equivalent(&self) {
        assert_eq!(self.adaptive.rows, self.fixed.rows);
        assert_eq!(
            self.adaptive.first_page, self.fixed.first_page,
            "fixed_plan={} adaptive_plan={}",
            self.fixed.pagination_plan, self.adaptive.pagination_plan
        );
        assert_eq!(
            self.adaptive.second_page, self.fixed.second_page,
            "fixed_plan={} adaptive_plan={}",
            self.fixed.pagination_plan, self.adaptive.pagination_plan
        );
        assert_eq!(
            self.adaptive.transaction_visible,
            self.fixed.transaction_visible
        );
        assert_eq!(
            self.adaptive.transaction_hidden,
            self.fixed.transaction_hidden
        );
        assert_eq!(self.adaptive.join_rows, self.fixed.join_rows);
        assert_eq!(self.adaptive.error, self.fixed.error);
        assert!(
            self.fixed
                .selected_plan
                .contains("adaptive_plan_enabled=false"),
            "plan={}",
            self.fixed.selected_plan
        );
        assert!(
            self.fixed.selected_plan.contains(BASE_INDEX),
            "plan={}",
            self.fixed.selected_plan
        );
        assert!(
            self.adaptive
                .selected_plan
                .contains("adaptive_reason=selected_operator_feedback"),
            "plan={}",
            self.adaptive.selected_plan
        );
        assert!(
            self.adaptive.selected_plan.contains(PREFERRED_INDEX),
            "plan={}",
            self.adaptive.selected_plan
        );
        assert!(
            self.adaptive
                .fallback_plan
                .contains("operator_feedback_reason=missing"),
            "plan={}",
            self.adaptive.fallback_plan
        );
        assert!(
            self.adaptive
                .fallback_plan
                .contains("adaptive_reason=no_runtime_observation"),
            "plan={}",
            self.adaptive.fallback_plan
        );
        assert!(
            self.adaptive.fallback_plan.contains(
                "adaptive_base_alternative=index:postgres.public.adaptive_profile_title_idx adaptive_selected_alternative=index:postgres.public.adaptive_profile_title_idx"
            ),
            "plan={}",
            self.adaptive.fallback_plan
        );
        assert_eq!(self.fixed.join_rows.len(), 2_047);
        assert!(
            self.fixed
                .join_plan
                .contains("operator_switch_enabled=false"),
            "plan={}",
            self.fixed.join_plan
        );
        assert!(
            self.adaptive
                .join_plan
                .contains("operator_switch_enabled=true"),
            "plan={}",
            self.adaptive.join_plan
        );
        assert_eq!(
            self.fixed.metrics["adaptive_candidates"]["operator_switch_successes"],
            0
        );
        assert_eq!(
            self.adaptive.metrics["adaptive_candidates"]["operator_switch_successes"],
            1
        );
    }

    pub fn write_requested_artifact(&self) {
        let Ok(path) = std::env::var("CASSIE_ADAPTIVE_EVIDENCE_PATH") else {
            return;
        };
        self.assert_equivalent();
        let artifact = json!({
            "schema_version": "cassie-adaptive-profile-evidence.v1",
            "immutable_commit": std::env::var("CASSIE_EVIDENCE_COMMIT").unwrap_or_else(|_| "local".to_string()),
            "fixture": {
                "id": "adaptive-profile-evidence-v1",
                "scalar_rows": 9,
                "join_left_rows": 2050,
                "join_right_rows": 2047,
                "table": TABLE,
                "base_index": BASE_INDEX,
                "preferred_index": PREFERRED_INDEX,
            },
            "selected_surface": [
                "feedback-informed scalar index selection",
                "vectorized-to-merge join switching diagnostics"
            ],
            "fixed": self.fixed.as_json(),
            "adaptive": self.adaptive.as_json(),
            "equivalent": true,
        });
        let path = Path::new(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create adaptive evidence directory");
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&artifact).expect("serialize adaptive evidence"),
        )
        .expect("write adaptive evidence artifact");
    }
}

impl ProfileObservation {
    fn as_json(&self) -> JsonValue {
        let join_bytes = serde_json::to_vec(&self.join_rows).expect("serialize join evidence rows");
        let join_digest = Sha256::digest(join_bytes).iter().fold(
            String::with_capacity(64),
            |mut encoded, byte| {
                write!(encoded, "{byte:02x}").expect("write digest byte");
                encoded
            },
        );
        json!({
            "profile": self.profile,
            "config": self.config,
            "results": {
                "rows": self.rows,
                "first_page": self.first_page,
                "second_page": self.second_page,
                "transaction_visible": self.transaction_visible,
                "transaction_hidden": self.transaction_hidden,
                "join_result": {
                    "row_count": self.join_rows.len(),
                    "sha256": join_digest,
                    "first_row": self.join_rows.first(),
                    "last_row": self.join_rows.last(),
                },
                "error": self.error,
            },
            "selected_plan": self.selected_plan,
            "pagination_plan": self.pagination_plan,
            "join_plan": self.join_plan,
            "fallback_plan": self.fallback_plan,
            "storage_reads": self.metrics["storage"]["data"]["reads"],
            "candidates": self.metrics["adaptive_candidates"],
            "memory": self.metrics["query"],
            "workers": self.metrics["runtime"]["active_operator_workers"],
            "fallback_reason": self.metrics["adaptive_candidates"]["last_plan_reason"],
        })
    }
}

fn disabled_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("disabled profile config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 256;
    config.limits.operator_feedback_enabled = false;
    config.limits.adaptive_execution_enabled = false;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
    config
}

fn evaluation_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("evaluation profile config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 256;
    config.limits.operator_feedback_enabled = true;
    config.limits.adaptive_execution_enabled = true;
    config.limits.adaptive_min_cost_savings_bps = 500;
    config.limits.adaptive_min_confidence_bps = 900;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::enabled();
    config.limits.operator_switch_join_row_threshold = 4_096;
    config
}

fn observe_profile(
    profile: &'static str,
    config: CassieRuntimeConfig,
    seed_feedback: bool,
) -> ProfileObservation {
    let path = evidence_path(profile);
    let config_evidence = config_json(&config);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config)
        .expect("create adaptive evidence Cassie");
    cassie.startup().expect("start adaptive evidence Cassie");
    let session = cassie.create_session("adaptive-evidence", None);
    seed_fixture(&cassie, &session);
    seed_join_fixture(&cassie, &session);
    if seed_feedback {
        seed_preferred_feedback(&cassie, &session);
    }

    let semantics = observe_semantics(&cassie, &session);
    let selected_plan = explain(&cassie, &session, SELECT_SQL);
    let pagination_plan = explain(
        &cassie,
        &session,
        "SELECT title, body, sequence FROM adaptive_profile_evidence ORDER BY title, body, sequence LIMIT 3 OFFSET 3",
    );
    let join_plan = explain(&cassie, &session, join_sql());
    let fallback_plan = explain(
        &cassie,
        &session,
        "SELECT title FROM adaptive_profile_evidence WHERE title = 'missing'",
    );
    let metrics = cassie.metrics();
    let observation = ProfileObservation {
        profile,
        rows: semantics.rows,
        first_page: semantics.first_page,
        second_page: semantics.second_page,
        transaction_visible: semantics.transaction_visible,
        transaction_hidden: semantics.transaction_hidden,
        join_rows: semantics.join_rows,
        error: semantics.error,
        selected_plan,
        pagination_plan,
        join_plan,
        fallback_plan,
        metrics,
        config: config_evidence,
    };
    drop(session);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
    observation
}

fn observe_semantics(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
) -> SemanticObservations {
    let rows = execute_rows(cassie, session, SELECT_SQL);
    let first_page = execute_rows(
        cassie,
        session,
        "SELECT title, body, sequence FROM adaptive_profile_evidence ORDER BY title, body, sequence LIMIT 3 OFFSET 0",
    );
    let second_page = execute_rows(
        cassie,
        session,
        "SELECT title, body, sequence FROM adaptive_profile_evidence ORDER BY title, body, sequence LIMIT 3 OFFSET 3",
    );
    cassie
        .execute_sql(session, "BEGIN", vec![])
        .expect("begin evidence transaction");
    cassie
        .execute_sql(
            session,
            "INSERT INTO adaptive_profile_evidence (title, body, sequence) VALUES ('pending', 'overlay', 10)",
            vec![],
        )
        .expect("insert evidence overlay");
    let transaction_visible = execute_rows(
        cassie,
        session,
        "SELECT title, body, sequence FROM adaptive_profile_evidence WHERE title = 'pending'",
    );
    let observer = cassie.create_session("adaptive-observer", None);
    let transaction_hidden = execute_rows(
        cassie,
        &observer,
        "SELECT title, body, sequence FROM adaptive_profile_evidence WHERE title = 'pending'",
    );
    cassie
        .execute_sql(session, "ROLLBACK", vec![])
        .expect("rollback evidence transaction");
    let mut join_rows = execute_rows(cassie, session, join_sql());
    join_rows.sort_by_key(|row| match row.first() {
        Some(Value::Int64(value)) => *value,
        _ => i64::MAX,
    });
    let error = cassie
        .execute_sql(
            session,
            "SELECT absent_column FROM adaptive_profile_evidence",
            vec![],
        )
        .expect_err("reject absent evidence column")
        .to_string();
    SemanticObservations {
        rows,
        first_page,
        second_page,
        transaction_visible,
        transaction_hidden,
        join_rows,
        error,
    }
}

fn execute_rows(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    sql: &str,
) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute adaptive evidence query")
        .rows
}

fn config_json(config: &CassieRuntimeConfig) -> JsonValue {
    json!({
        "operator_feedback_enabled": config.limits.operator_feedback_enabled,
        "vectorized_joins_enabled": config.limits.vectorized_joins_enabled,
        "vectorized_join_batch_size": config.limits.vectorized_join_batch_size,
        "adaptive_execution_enabled": config.limits.adaptive_execution_enabled,
        "adaptive_min_cost_savings_bps": config.limits.adaptive_min_cost_savings_bps,
        "adaptive_min_confidence_bps": config.limits.adaptive_min_confidence_bps,
        "operator_switching_enabled": config.limits.operator_switching_enabled.is_enabled(),
        "operator_switch_join_row_threshold": config.limits.operator_switch_join_row_threshold,
        "query_memory_budget_bytes": config.limits.query_memory_budget_bytes,
        "max_query_workers": config.limits.max_query_workers,
    })
}

fn seed_fixture(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {TABLE} (title TEXT, body TEXT, sequence BIGINT)"),
            vec![],
        )
        .expect("create adaptive evidence table");
    let values = (0..8)
        .map(|sequence| format!("('alpha', 'one', {sequence})"))
        .chain(std::iter::once("('beta', 'two', 8)".to_string()))
        .collect::<Vec<_>>()
        .join(", ");
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {TABLE} (title, body, sequence) VALUES {values}"),
            vec![],
        )
        .expect("seed adaptive evidence rows");
    for (index, field) in [(BASE_INDEX, "body"), (PREFERRED_INDEX, "title")] {
        cassie
            .execute_sql(
                session,
                &format!("CREATE INDEX {index} ON {TABLE} USING btree ({field})"),
                vec![],
            )
            .expect("create adaptive evidence index");
    }
}

fn seed_join_fixture(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE adaptive_profile_users (user_key BIGINT, name TEXT)",
            vec![],
        )
        .expect("create adaptive evidence users");
    cassie
        .execute_sql(
            session,
            "CREATE TABLE adaptive_profile_orders (order_user_key BIGINT, total BIGINT)",
            vec![],
        )
        .expect("create adaptive evidence orders");
    insert_join_rows(cassie, session, "adaptive_profile_users", 2_050, |index| {
        format!("({index}, 'user-{index:04}')")
    });
    insert_join_rows(cassie, session, "adaptive_profile_orders", 2_047, |index| {
        format!("({index}, {})", index * 10)
    });
}

fn insert_join_rows(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
    count: usize,
    row: impl Fn(usize) -> String,
) {
    for start in (0..count).step_by(256) {
        let end = start.saturating_add(256).min(count);
        let values = (start..end).map(&row).collect::<Vec<_>>().join(", ");
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {table} VALUES {values}"),
                vec![],
            )
            .expect("seed adaptive join rows");
    }
}

fn join_sql() -> &'static str {
    "SELECT adaptive_profile_users.user_key, adaptive_profile_users.name, adaptive_profile_orders.total FROM adaptive_profile_users JOIN adaptive_profile_orders ON adaptive_profile_users.user_key = adaptive_profile_orders.order_user_key"
}

fn seed_preferred_feedback(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for (index, elapsed_ms, reads) in [(BASE_INDEX, 90, 24), (PREFERRED_INDEX, 5, 1)] {
        let key: RuntimeFeedbackKey = cassie
            .read_operator_feedback_key_for_diagnostics(session, SELECT_SQL, Some(index))
            .expect("adaptive evidence feedback key");
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(
                    &key,
                    &RuntimeFeedbackObservation {
                        rows_in: reads,
                        rows_out: 8,
                        elapsed_ms,
                        storage_reads: reads,
                        ..RuntimeFeedbackObservation::default()
                    },
                )
                .expect("seed adaptive evidence feedback");
        }
    }
}

fn explain(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> String {
    cassie
        .execute_sql(session, &format!("EXPLAIN {sql}"), vec![])
        .expect("explain adaptive evidence query")
        .rows[0][0]
        .as_str()
        .expect("textual adaptive evidence plan")
        .to_string()
}

fn evidence_path(profile: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-adaptive-evidence-{profile}-{}",
        Uuid::new_v4()
    ))
}
