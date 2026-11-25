//! A projector correction (endpoint fix, previously missing vertex, mapping-dependency
//! rename) in one transaction: the writer, a separate reader and both graphs see exactly the
//! documented rows after a rollback run and after a commit run (#1351).

use crate::fixture::GraphFixture;
use crate::identity_fixture::{
    graph_edges, member_key, run_as, started_authz, state_of, vertex_ref, Storage, STORAGES,
};
use cassie::app::CassieSession;

const BEFORE: [&str; 5] = [
    "1 | 1 | ann | 1 | bob",
    "2 | 1 | ann | 1 | bob",
    "3 | 1 | bob | 1 | bob",
    "4 | 2 | ann | 2 | bob",
    "6 | 2 | bob | 2 | ann",
];

const AFTER: [&str; 7] = [
    "1 | 1 | ann | 1 | bob",
    "2 | 1 | ann | 1 | bob",
    "3 | 1 | bob | 1 | bob",
    "4 | 2 | ann | 2 | bob",
    "5 | 1 | ann | 1 | bob",
    "6 | 2 | bob | 2 | ann",
    "7 | 2 | bob | 2 | cy",
];

const CORRECTIONS: [&str; 4] = [
    "UPDATE membership SET dst = 'bob' WHERE id = 5",
    "INSERT INTO member VALUES (2, 'cy', 'Cy two')",
    "ALTER TABLE membership RENAME COLUMN dst TO target_code",
    "ALTER TABLE member RENAME TO account",
];

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_owned()).collect()
}

fn views(fixture: &GraphFixture, session: &CassieSession) -> (Vec<String>, Vec<String>) {
    (
        graph_edges(fixture, session, "authz_a"),
        graph_edges(fixture, session, "authz_b"),
    )
}

fn new_member_state(fixture: &GraphFixture, session: &CassieSession) -> String {
    match fixture.cassie.execute_sql(
        session,
        "SELECT cassie_graph.vertex_ref('authz_a', 'm', ARRAY[cassie_graph.key_part(2::bigint), \
         cassie_graph.key_part('cy'::text)])",
        Vec::new(),
    ) {
        Ok(_) => "visible".to_owned(),
        Err(error) => error.sql_state().to_owned(),
    }
}

/// Runs the corrections in the writer's transaction and ends it with `finish`.
fn correct_then(storage: Storage, finish: &str) -> [(Vec<String>, Vec<String>, String); 4] {
    let fixture = started_authz("graph_identity_corrections", storage);
    let writer = &fixture.session;
    let reader = fixture.cassie.create_session("tester", None);
    run_as(&fixture.cassie, writer, "BEGIN");
    for statement in CORRECTIONS {
        run_as(&fixture.cassie, writer, statement);
    }
    let writer_inside = views(&fixture, writer);
    let reader_inside = views(&fixture, &reader);
    let new_member = (
        new_member_state(&fixture, writer),
        new_member_state(&fixture, &reader),
    );
    run_as(&fixture.cassie, writer, finish);
    let writer_after = views(&fixture, writer);
    let reader_after = views(&fixture, &reader);
    let after_state = (
        new_member_state(&fixture, writer),
        new_member_state(&fixture, &reader),
    );
    [
        (writer_inside.0, writer_inside.1, new_member.0),
        (reader_inside.0, reader_inside.1, new_member.1),
        (writer_after.0, writer_after.1, after_state.0),
        (reader_after.0, reader_after.1, after_state.1),
    ]
}

fn expected(rows: &[&str], member: &str) -> (Vec<String>, Vec<String>, String) {
    (lines(rows), lines(rows), member.to_owned())
}

#[test]
fn should_restore_the_original_rows_on_rollback() {
    for storage in STORAGES {
        // Arrange
        let finish = "ROLLBACK";

        // Act
        let observed = correct_then(storage, finish);

        // Assert
        assert_eq!(
            (storage.name(), observed),
            (
                storage.name(),
                [
                    expected(&AFTER, "visible"),
                    expected(&BEFORE, "42704"),
                    expected(&BEFORE, "42704"),
                    expected(&BEFORE, "42704"),
                ]
            )
        );
    }
}

#[test]
fn should_show_the_corrected_rows_to_the_writer_then_the_reader_on_commit() {
    for storage in STORAGES {
        // Arrange
        let finish = "COMMIT";

        // Act
        let observed = correct_then(storage, finish);

        // Assert
        assert_eq!(
            (storage.name(), observed),
            (
                storage.name(),
                [
                    expected(&AFTER, "visible"),
                    expected(&BEFORE, "42704"),
                    expected(&AFTER, "visible"),
                    expected(&AFTER, "visible"),
                ]
            )
        );
    }
}

#[test]
fn should_resolve_the_created_vertex_only_in_its_own_tenant() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_new_vertex", storage);
        run_as(
            &fixture.cassie,
            &fixture.session,
            "INSERT INTO member VALUES (2, 'cy', 'Cy two')",
        );

        // Act
        let created = vertex_ref(&fixture, "authz_b", "person", &member_key(2, "cy"));
        let other_tenant = state_of(
            &fixture.cassie,
            &fixture.session,
            "SELECT cassie_graph.vertex_ref('authz_b', 'person', ARRAY[\
             cassie_graph.key_part(1::bigint), cassie_graph.key_part('cy'::text)])",
        );

        // Assert
        assert_eq!(created.key, member_key(2, "cy"));
        assert_eq!(other_tenant, "42704");
    }
}
