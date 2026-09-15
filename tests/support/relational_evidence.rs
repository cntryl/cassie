use std::cmp::Ordering;

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql::{data_dir, use_local_storage};

#[derive(Clone)]
struct Fact {
    id: i64,
    category: Option<String>,
    score: Option<i64>,
}

pub struct RelationalComparison {
    pub keyed_join: Vec<Vec<Value>>,
    pub filtered_cross_join: Vec<Vec<Value>>,
    pub cte: Vec<Vec<Value>>,
    pub direct: Vec<Vec<Value>>,
    pub window: Vec<Vec<Value>>,
    pub expected_window: Vec<Vec<Value>>,
    pub has_null_window_value: bool,
    pub has_tied_window_rank: bool,
}

pub struct SeededRelationalFixture {
    seed: u64,
    reverse_insertion: bool,
}

impl SeededRelationalFixture {
    pub const fn from_seed(seed: u64, reverse_insertion: bool) -> Self {
        Self {
            seed,
            reverse_insertion,
        }
    }

    pub fn execute(&self) -> RelationalComparison {
        use_local_storage();
        let path = data_dir("seeded-relational-evidence");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("relational evidence runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("relational evidence Cassie");
            cassie.startup().expect("start relational evidence Cassie");
            let session = cassie.create_session("relational-evidence", None);
            execute(
                &cassie,
                &session,
                "CREATE TABLE evidence_facts (ordinal BIGINT, category TEXT, score BIGINT)",
            );
            execute(
                &cassie,
                &session,
                "CREATE TABLE evidence_categories (category TEXT, label TEXT)",
            );
            execute(
                &cassie,
                &session,
                "INSERT INTO evidence_categories (category, label) VALUES ('a', 'alpha'), ('b', 'beta'), ('c', 'gamma')",
            );

            let facts = seeded_facts(self.seed, 12);
            let mut insertion = facts.clone();
            if self.reverse_insertion {
                insertion.reverse();
            }
            execute(
                &cassie,
                &session,
                &format!(
                    "INSERT INTO evidence_facts (ordinal, category, score) VALUES {}",
                    insertion
                        .iter()
                        .map(fact_sql)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );

            let keyed_join = query(
                &cassie,
                &session,
                "SELECT evidence_facts.ordinal, evidence_categories.label, evidence_facts.score FROM evidence_facts JOIN evidence_categories ON evidence_facts.category = evidence_categories.category ORDER BY evidence_facts.ordinal",
            );
            let filtered_cross_join = query(
                &cassie,
                &session,
                "SELECT evidence_facts.ordinal, evidence_categories.label, evidence_facts.score FROM evidence_facts JOIN evidence_categories ON true WHERE evidence_facts.category = evidence_categories.category ORDER BY evidence_facts.ordinal",
            );
            let cte = query(
                &cassie,
                &session,
                "WITH selected AS (SELECT ordinal, category, score FROM evidence_facts WHERE score >= 0) SELECT ordinal, category, score FROM selected ORDER BY ordinal",
            );
            let direct = query(
                &cassie,
                &session,
                "SELECT ordinal, category, score FROM evidence_facts WHERE score >= 0 ORDER BY ordinal",
            );
            let window = query(
                &cassie,
                &session,
                "SELECT category, ordinal, score, rank() OVER (PARTITION BY category ORDER BY score DESC NULLS LAST) AS ranking FROM evidence_facts ORDER BY category ASC NULLS LAST, ranking ASC, ordinal ASC",
            );
            let expected_window = expected_window(&facts);
            let has_null_window_value = window
                .iter()
                .any(|row| row.get(2) == Some(&Value::Null));
            let has_tied_window_rank = window.windows(2).any(|rows| {
                rows[0].first() == rows[1].first() && rows[0].get(3) == rows[1].get(3)
            });

            let _ = std::fs::remove_dir_all(path);
            RelationalComparison {
                keyed_join,
                filtered_cross_join,
                cte,
                direct,
                window,
                expected_window,
                has_null_window_value,
                has_tied_window_rank,
            }
        })
    }
}

fn execute(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute relational evidence statement");
}

fn query(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("query relational evidence")
        .rows
}

fn fact_sql(fact: &Fact) -> String {
    let category = fact
        .category
        .as_ref()
        .map_or_else(|| "NULL".to_owned(), |value| format!("'{value}'"));
    let score = fact
        .score
        .map_or_else(|| "NULL".to_owned(), |value| value.to_string());
    format!("({}, {category}, {score})", fact.id)
}

fn seeded_facts(seed: u64, count: usize) -> Vec<Fact> {
    let mut state = seed;
    (0..count)
        .map(|index| {
            let mixed = next_mixed(&mut state);
            let category = match index {
                0 => None,
                1..=3 => Some("a".to_owned()),
                _ => Some(
                    ["a", "b", "c"][usize::try_from(mixed % 3).expect("bounded category")]
                        .to_owned(),
                ),
            };
            let score = match index {
                0 | 3 => None,
                1 | 2 => Some(7),
                _ => Some(i64::try_from(mixed % 21).expect("bounded score") - 10),
            };
            Fact {
                id: i64::try_from(index).expect("bounded fact id"),
                category,
                score,
            }
        })
        .collect()
}

fn expected_window(facts: &[Fact]) -> Vec<Vec<Value>> {
    let mut ordered = facts.to_vec();
    ordered.sort_by(|left, right| {
        category_order(left.category.as_ref(), right.category.as_ref())
            .then_with(|| score_desc_nulls_last(left.score, right.score))
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut previous_category: Option<Option<String>> = None;
    let mut previous_score: Option<Option<i64>> = None;
    let mut partition_index = 0_usize;
    let mut rank = 0_usize;
    ordered
        .into_iter()
        .map(|fact| {
            if previous_category.as_ref() == Some(&fact.category) {
                partition_index += 1;
                if previous_score != Some(fact.score) {
                    rank = partition_index + 1;
                    previous_score = Some(fact.score);
                }
            } else {
                partition_index = 0;
                rank = 1;
                previous_category = Some(fact.category.clone());
                previous_score = Some(fact.score);
            }
            vec![
                fact.category.map_or(Value::Null, Value::String),
                Value::Int64(fact.id),
                fact.score.map_or(Value::Null, Value::Int64),
                Value::Int64(i64::try_from(rank).expect("bounded rank")),
            ]
        })
        .collect()
}

fn category_order(left: Option<&String>, right: Option<&String>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn score_desc_nulls_last(left: Option<i64>, right: Option<i64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn next_mixed(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = *state;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}
