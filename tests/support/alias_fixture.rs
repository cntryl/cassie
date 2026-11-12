//! The reader identity used by the finite alias authorization fixture.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieSession;

/// Creates and authenticates the alias fixture reader at the call site.
///
/// # Panics
/// Panics when fixture role creation or authentication fails.
pub fn reader(fixture: &SqlFixture) -> CassieSession {
    fixture
        .cassie
        .create_role("alias_reader", true, Some("fixture-password".into()), false)
        .expect("reader role");
    fixture
        .cassie
        .authenticate_role("alias_reader", Some("fixture-password"), None)
        .expect("authenticate reader")
}

/// Creates the indexed relation and an explicitly authorized reader.
///
/// # Panics
/// Panics when startup, fixture setup, role creation, or grant publication fails.
pub fn indexed_pagination_fixture() -> (SqlFixture, CassieSession) {
    let fixture = sql_fixture("alias_indexed_pagination", &[]);
    fixture.cassie.startup().expect("startup Cassie");
    let root = fixture
        .cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("authenticated administrator");
    for sql in [
        "CREATE TABLE public.records (score BIGINT NOT NULL)",
        "INSERT INTO public.records VALUES (10), (20), (30), (40)",
        "CREATE INDEX records_score_idx ON public.records USING btree (score)",
    ] {
        fixture
            .cassie
            .execute_sql(&root, sql, vec![])
            .expect("fixture setup");
    }
    let reader = reader(&fixture);
    for sql in [
        "GRANT USAGE ON SCHEMA public TO alias_reader",
        "GRANT SELECT ON TABLE public.records TO alias_reader",
    ] {
        fixture
            .cassie
            .execute_sql(&root, sql, vec![])
            .expect("reader grant");
    }
    (fixture, reader)
}
