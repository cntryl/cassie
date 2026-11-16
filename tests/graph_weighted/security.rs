//! Read authority: a graph grants nothing, every table the call can read needs SELECT.

use crate::world::{fast, slow, vertex, Wg};

fn reader(world: &Wg) -> cassie::app::CassieSession {
    let password = crate::support_sql::test_root_password();
    world.run(&format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"));
    world
        .fixture
        .cassie
        .authenticate_role("reader", Some(password), Some("postgres".to_owned()))
        .expect("role login")
}

fn sql(from: i64, to: i64, rest: &str) -> String {
    Wg::select(&format!("'wg', {}, {}, {rest}", vertex(from), vertex(to)))
}

fn state(world: &Wg, session: &cassie::app::CassieSession, sql: &str) -> String {
    match world.fixture.cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => "success".to_owned(),
        Err(error) => error.sql_state().to_owned(),
    }
}

#[test]
fn should_deny_a_role_without_select_on_the_mapped_tables_with_42501() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), slow(2, 2, 3, 1)]);
    let session = reader(&world);

    // Act
    let result = state(&world, &session, &sql(1, 3, "'weight', 2"));

    // Assert
    assert_eq!(result, "42501");
}

#[test]
fn should_deny_a_present_or_missing_element_alike_without_select() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1)]);
    let session = reader(&world);

    // Act
    let present = state(&world, &session, &sql(1, 2, "'weight', 1"));
    let absent = state(
        &world,
        &session,
        &sql(1, 3, "'weight', 1, 'out', NULL, NULL, 1"),
    );

    // Assert
    assert_eq!((present.as_str(), absent.as_str()), ("42501", "42501"));
}

#[test]
fn should_require_select_only_on_the_tables_of_the_selected_labels() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), slow(2, 2, 3, 1)]);
    let session = reader(&world);
    world.run_as_root("GRANT USAGE ON SCHEMA public TO reader");
    world.run_as_root("GRANT SELECT ON TABLE public.v TO reader");
    world.run_as_root("GRANT SELECT ON TABLE public.fast TO reader");

    // Act
    let allowed = state(
        &world,
        &session,
        &sql(1, 2, "'weight', 1, 'out', ARRAY['fast']"),
    );
    let denied = state(&world, &session, &sql(1, 2, "'weight', 1"));

    // Assert
    assert_eq!((allowed.as_str(), denied.as_str()), ("success", "42501"));
}

#[test]
fn should_allow_a_role_with_select_on_every_mapped_table() {
    // Arrange
    let world = Wg::new(3, &[fast(1, 1, 2, 1), slow(2, 2, 3, 1)]);
    let session = reader(&world);
    world.run_as_root("GRANT USAGE ON SCHEMA public TO reader");
    for table in ["v", "fast", "slow"] {
        world.run_as_root(&format!("GRANT SELECT ON TABLE public.{table} TO reader"));
    }

    // Act
    let result = state(&world, &session, &sql(1, 3, "'weight', 2"));

    // Assert
    assert_eq!(result, "success");
}
