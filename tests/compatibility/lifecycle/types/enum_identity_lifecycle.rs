use super::*;

#[derive(Debug, PartialEq)]
struct EnumIdentityState {
    type_oid: cassie::types::Value,
    array_oid: cassie::types::Value,
    labels: Vec<Vec<cassie::types::Value>>,
    values: Vec<Vec<cassie::types::Value>>,
}

fn seed_enum_identity_fixture(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    schema: &str,
) {
    for statement in [
        format!("CREATE SCHEMA {schema}"),
        format!("CREATE TYPE {schema}.state_code AS ENUM ('waiting', 'ready', 'done')"),
        format!("CREATE TABLE {schema}.jobs (id INT PRIMARY KEY, state {schema}.state_code)"),
        format!("INSERT INTO {schema}.jobs VALUES (1, 'waiting'), (2, 'done')"),
        format!("ALTER TYPE {schema}.state_code ADD VALUE 'paused' BEFORE 'ready'"),
        format!("ALTER TYPE {schema}.state_code RENAME VALUE 'done' TO 'complete'"),
        format!("ALTER TYPE {schema}.state_code RENAME TO task_state"),
        format!("INSERT INTO {schema}.jobs VALUES (3, 'paused')"),
    ] {
        cassie
            .execute_sql(session, &statement, vec![])
            .unwrap_or_else(|error| panic!("execute enum lifecycle setup {statement}: {error}"));
    }
}

fn read_enum_identity_state(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    schema: &str,
) -> EnumIdentityState {
    let identity = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT oid::BIGINT, typarray::BIGINT FROM pg_catalog.pg_type \
                 WHERE oid = '{schema}.task_state'::regtype"
            ),
            vec![],
        )
        .expect("read enum and array identities")
        .rows()[0]
        .clone();
    let labels = cassie
        .execute_sql(
            session,
            &format!("SELECT enum_range(NULL::{schema}.task_state)::TEXT"),
            vec![],
        )
        .expect("read enum label order")
        .rows()
        .to_vec();
    let values = cassie
        .execute_sql(
            session,
            &format!("SELECT id, state::TEXT FROM {schema}.jobs ORDER BY state"),
            vec![],
        )
        .expect("read persisted values in enum order")
        .rows()
        .to_vec();
    EnumIdentityState {
        type_oid: identity[0].clone(),
        array_oid: identity[1].clone(),
        labels,
        values,
    }
}

fn collect_enum_identity_state(path: &str, schema: &str, initialize: bool) -> EnumIdentityState {
    let cassie = Cassie::new_with_data_dir(path).expect("open local Cassie instance");
    cassie.startup().expect("start local Cassie instance");
    let session = cassie.create_session("tester", None);
    if initialize {
        seed_enum_identity_fixture(&cassie, &session, schema);
    }
    let state = read_enum_identity_state(&cassie, &session, schema);
    drop(session);
    drop(cassie);
    state
}
