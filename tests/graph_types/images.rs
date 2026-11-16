//! Database images and graph reference identity (#1268): restore into a distinct database.

use crate::fixture::{started, GraphFixture, SOCIAL_SETUP};

fn social(label: &str) -> GraphFixture {
    started(label, &SOCIAL_SETUP)
}

fn image_of(fixture: &GraphFixture, database: &str) -> Vec<u8> {
    let mut backup = fixture
        .cassie
        .begin_database_backup(database)
        .expect("backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("stream") {
        image.extend(chunk);
    }
    image
}

fn restore(fixture: &GraphFixture, image: &[u8], target: &str) {
    let mut session = fixture
        .cassie
        .begin_database_restore(target)
        .expect("begin restore");
    session.push_chunk(image).expect("push");
    session.finish().expect("finish");
}

const REF_DB: &str = "SELECT (r).database_id, (r).graph_id, (r).mapping_id FROM (SELECT \
     cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(2::bigint)]) AS r) t";

fn clone_of(fixture: &GraphFixture, label: &str) -> cassie::app::CassieSession {
    let image = image_of(fixture, "postgres");
    restore(fixture, &image, label);
    fixture
        .cassie
        .create_session("tester", Some(label.to_owned()))
}

fn run_in(fixture: &GraphFixture, session: &cassie::app::CassieSession, sql: &str) {
    assert!(
        fixture.cassie.execute_sql(session, sql, vec![]).is_ok(),
        "a statement failed in the clone"
    );
}

#[test]
fn should_not_resolve_a_source_graph_in_a_distinct_database_clone() {
    // Arrange
    let fixture = social("graph_images_unresolved");

    // Act
    let clone = clone_of(&fixture, "clone_a");
    let outcome = fixture.cassie.execute_sql(&clone, REF_DB, vec![]);

    // Assert
    assert_eq!(outcome.expect_err("no graph in the clone").sql_state(), "42704");
    assert!(fixture.execute(REF_DB).is_ok());
}

#[test]
fn should_give_a_distinct_clone_a_fresh_database_identity_and_new_graph_identities() {
    // Arrange
    let fixture = social("graph_images_fresh");
    let source = fixture.rows(REF_DB);
    let clone = clone_of(&fixture, "clone_b");

    // Act
    run_in(&fixture, &clone, SOCIAL_SETUP[4]);
    let cloned = fixture
        .cassie
        .execute_sql(&clone, REF_DB, vec![])
        .expect("reference in the clone")
        .rows()
        .to_vec();

    // Assert
    assert_ne!(source[0][0], cloned[0][0]);
    assert_ne!(source[0][1], cloned[0][1]);
    assert_ne!(source[0][2], cloned[0][2]);
}

#[test]
fn should_keep_the_source_identities_after_a_clone_is_taken() {
    // Arrange
    let fixture = social("graph_images_source");
    let before = fixture.rows(REF_DB);

    // Act
    let clone = clone_of(&fixture, "clone_c");
    run_in(&fixture, &clone, SOCIAL_SETUP[4]);

    // Assert
    assert_eq!(fixture.rows(REF_DB), before);
}
