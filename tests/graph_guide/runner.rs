//! Runs the examples of one guide world against a fresh in-memory instance. A failure names
//! the example and the kind of mismatch only, never SQL text or result values.

use std::collections::HashMap;

use crate::guide::{Example, Expect};
use crate::support_sql;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieSession;
use cassie::types::Value;

/// Worlds that need a login role created before their examples run.
const ROLE_WORLDS: [&str; 1] = ["permissions"];
const READER: &str = "reader";

pub fn fixture_for(world: &str) -> SqlFixture {
    let fixture = sql_fixture(&format!("graph-guide-{world}"), &[]);
    if ROLE_WORLDS.contains(&world) {
        let password = support_sql::test_root_password();
        let created =
            fixture.execute_as_root(&format!("CREATE ROLE {READER} LOGIN PASSWORD '{password}'"));
        assert!(created.is_ok(), "the guide fixture role is created");
    }
    fixture
}

fn render(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some("NULL".to_owned()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Int64(number) => Some(number.to_string()),
        Value::Float64(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        Value::Json(json) => Some(json.to_string()),
        _ => None,
    }
}

pub fn render_rows(rows: &[Vec<Value>]) -> Option<Vec<String>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(render)
                .collect::<Option<Vec<_>>>()
                .map(|cells| cells.join(" | "))
        })
        .collect()
}

struct Sessions<'a> {
    fixture: &'a SqlFixture,
    named: HashMap<String, CassieSession>,
}

impl<'a> Sessions<'a> {
    fn new(fixture: &'a SqlFixture) -> Self {
        Self {
            fixture,
            named: HashMap::new(),
        }
    }

    fn get(&mut self, name: &str) -> &CassieSession {
        if name == "main" {
            return &self.fixture.session;
        }
        self.named.entry(name.to_owned()).or_insert_with(|| {
            let cassie = &self.fixture.cassie;
            match name {
                "root" => cassie.create_session("root", None),
                READER => cassie
                    .authenticate_role(
                        READER,
                        Some(support_sql::test_root_password()),
                        Some("postgres".to_owned()),
                    )
                    .expect("the guide role logs in"),
                _ => cassie.create_session("tester", None),
            }
        })
    }
}

fn check(example: &Example, outcome: &Result<Vec<Vec<Value>>, String>) -> Option<String> {
    match (&example.expect, outcome) {
        (Expect::Success, Ok(_)) => None,
        (Expect::Success, Err(state)) => {
            Some(format!("{}: failed with SQLSTATE {state}", example.id))
        }
        (Expect::State(expected), Err(state)) if expected == state => None,
        (Expect::State(expected), Err(state)) => Some(format!(
            "{}: expected SQLSTATE {expected}, observed {state}",
            example.id
        )),
        (Expect::State(expected), Ok(_)) => Some(format!(
            "{}: expected SQLSTATE {expected}, observed success",
            example.id
        )),
        (Expect::Rows(expected), Ok(rows)) => match render_rows(rows) {
            None => Some(format!(
                "{}: result has a value the guide cannot render",
                example.id
            )),
            Some(observed) if &observed == expected => None,
            Some(observed) => Some(format!(
                "{}: documented {} rows, observed {} rows; first difference at line {}",
                example.id,
                expected.len(),
                observed.len(),
                observed
                    .iter()
                    .zip(expected)
                    .position(|(left, right)| left != right)
                    .map_or(observed.len().min(expected.len()) + 1, |index| index + 1)
            )),
        },
        (Expect::Rows(_), Err(state)) => {
            Some(format!("{}: failed with SQLSTATE {state}", example.id))
        }
    }
}

/// Runs every example of `world` in document order and returns the failures.
pub fn run_world(world: &str, examples: &[Example]) -> Vec<String> {
    let fixture = fixture_for(world);
    let mut sessions = Sessions::new(&fixture);
    let mut failures = Vec::new();
    for example in examples.iter().filter(|example| example.world == world) {
        let session = sessions.get(&example.session);
        let outcome = fixture
            .cassie
            .execute_sql(session, &example.sql, example.params.clone())
            .map(|result| result.rows().to_vec())
            .map_err(|error| error.sql_state().to_owned());
        if let Some(failure) = check(example, &outcome) {
            failures.push(failure);
        }
    }
    failures
}
