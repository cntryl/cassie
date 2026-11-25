//! A reader of one mapped source who lacks SELECT on another cannot reach the inaccessible
//! rows through a second graph, a second alias or a reference (#1351).

use crate::fixture::GraphFixture;
use crate::identity_fixture::{
    graph_edges, member_key, ref_sql, render, run_as, started_authz, state_of, STORAGES,
};
use cassie::app::CassieSession;
use cassie::types::graph::{ElementKind, KeyPart};

/// A fixture whose `reader` role holds SELECT on `member` only.
fn restricted(
    label: &str,
    storage: crate::identity_fixture::Storage,
) -> (GraphFixture, CassieSession) {
    let fixture = started_authz(label, storage);
    let password = crate::support_sql::test_root_password();
    let root = fixture.cassie.create_session("root", None);
    for sql in [
        format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"),
        "GRANT USAGE ON SCHEMA public TO reader".to_owned(),
        "GRANT SELECT ON TABLE public.member TO reader".to_owned(),
    ] {
        run_as(&fixture.cassie, &root, &sql);
    }
    let reader = fixture
        .cassie
        .authenticate_role("reader", Some(password), Some("postgres".to_owned()))
        .expect("role login");
    (fixture, reader)
}

fn edge_query(graph: &str) -> String {
    format!(
        "SELECT g.id FROM GRAPH_TABLE ({graph} MATCH (s IS member) -[e IS grants]-> (d IS member) \
         COLUMNS (e.id AS id)) AS g"
    )
}

#[test]
fn should_deny_the_edge_pattern_of_every_graph_without_select_on_the_edge_source() {
    for storage in STORAGES {
        // Arrange
        let (fixture, reader) = restricted("graph_identity_denied", storage);

        // Act
        let states = ["authz_a", "authz_b"]
            .map(|graph| state_of(&fixture.cassie, &reader, &edge_query(graph)));

        // Assert
        assert_eq!(
            (storage.name(), states),
            (storage.name(), ["42501".to_owned(), "42501".to_owned()])
        );
    }
}

#[test]
fn should_deny_edge_references_without_revealing_existence() {
    for storage in STORAGES {
        // Arrange
        let (fixture, reader) = restricted("graph_identity_ref_denied", storage);
        let edge = |graph: &str, mapping: &str, id: i64| {
            ref_sql(ElementKind::Edge, graph, mapping, &[KeyPart::BigInt(id)])
        };
        let vertex = |graph: &str, mapping: &str, tenant: i64, code: &str| {
            ref_sql(
                ElementKind::Vertex,
                graph,
                mapping,
                &member_key(tenant, code),
            )
        };
        let path = format!(
            "SELECT s.position FROM cassie_graph.path_steps(ROW(ARRAY[{}, {}], ARRAY[{}], \
             ARRAY[1::smallint])::cassie_graph.graph_path) AS s",
            vertex("authz_b", "person", 1, "ann"),
            vertex("authz_b", "person", 1, "bob"),
            edge("authz_b", "grants_to", 1)
        );

        // Act
        let states = [
            format!("SELECT {}", edge("authz_a", "grant_edge", 1)),
            format!("SELECT {}", edge("authz_a", "grant_edge", 404)),
            format!("SELECT {}", edge("authz_b", "grants_to", 1)),
            path,
        ]
        .map(|sql| state_of(&fixture.cassie, &reader, &sql));

        // Assert
        assert_eq!(
            (storage.name(), states),
            (storage.name(), ["42501"; 4].map(str::to_owned))
        );
    }
}

#[test]
fn should_let_the_reader_use_the_readable_source_through_either_graph() {
    for storage in STORAGES {
        // Arrange
        let (fixture, reader) = restricted("graph_identity_readable", storage);
        let vertices = |graph: &str, label: &str| {
            format!(
                "SELECT g.tenant_id, g.code FROM GRAPH_TABLE ({graph} MATCH (v IS {label}) \
                 COLUMNS (v.tenant_id AS tenant_id, v.code AS code)) AS g \
                 ORDER BY g.tenant_id, g.code"
            )
        };

        // Act
        let observed = ["authz_a", "authz_b"].map(|graph| {
            match fixture
                .cassie
                .execute_sql(&reader, &vertices(graph, "member"), Vec::new())
            {
                Ok(result) => render(result.rows()),
                Err(error) => vec![format!("SQLSTATE {}", error.sql_state())],
            }
        });
        let reference = match fixture.cassie.execute_sql(
            &reader,
            &format!(
                "SELECT {}",
                ref_sql(ElementKind::Vertex, "authz_a", "m", &member_key(2, "ann"))
            ),
            Vec::new(),
        ) {
            Ok(_) => "resolved".to_owned(),
            Err(error) => error.sql_state().to_owned(),
        };

        // Assert
        let expected = ["1 | ann", "1 | bob", "2 | ann", "2 | bob"].map(str::to_owned);
        assert_eq!(
            (storage.name(), observed, reference),
            (
                storage.name(),
                [expected.to_vec(), expected.to_vec()],
                "resolved".to_owned()
            )
        );
    }
}

#[test]
fn should_expose_the_edges_only_after_select_on_the_edge_source_is_granted() {
    for storage in STORAGES {
        // Arrange
        let (fixture, reader) = restricted("graph_identity_granted", storage);
        let before = state_of(&fixture.cassie, &reader, &edge_query("authz_b"));
        run_as(
            &fixture.cassie,
            &fixture.cassie.create_session("root", None),
            "GRANT SELECT ON TABLE public.membership TO reader",
        );

        // Act
        let after = (
            graph_edges(&fixture, &reader, "authz_a"),
            graph_edges(&fixture, &reader, "authz_b"),
        );

        // Assert
        assert_eq!(before, "42501");
        assert_eq!(after.0.len(), 5);
        assert_eq!(after.0, after.1);
    }
}
