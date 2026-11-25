//! Started memory fixtures for query identity and native function contracts.

use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};

/// Starts before creating the scoped session and seeding its objects.
///
/// # Panics
///
/// Panics when fixture startup, namespace selection or a setup statement fails.
#[must_use]
pub fn started(label: &str, setup: &[&str]) -> SqlFixture {
    let config = cassie::config::CassieRuntimeConfig {
        password: uuid::Uuid::new_v4().to_string(),
        ..cassie::config::CassieRuntimeConfig::default()
    };
    let mut fixture = sql_fixture_with_config(label, &[], config);
    fixture.cassie.startup().expect("start query fixture");
    fixture.session = fixture.cassie.create_session("tester", None);
    fixture
        .execute("SET search_path TO public")
        .expect("public namespace");
    for sql in setup {
        fixture.execute(sql).expect("seed query fixture");
    }
    fixture
}
