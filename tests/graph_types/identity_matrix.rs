//! Element identity and visible edges of a tenant-qualified composite key mapped into two
//! named graphs and two aliases, against an independent relational join (#1351).

use crate::identity_fixture::{
    expected_identities, graph_edge_keys, graph_edges, graph_identities, join_edges, member_key,
    reference, started_authz, state_of, vertex_ref, STORAGES,
};
use cassie::types::graph::{ElementKind, KeyPart};

const VISIBLE: [&str; 5] = [
    "1 | 1 | ann | 1 | bob",
    "2 | 1 | ann | 1 | bob",
    "3 | 1 | bob | 1 | bob",
    "4 | 2 | ann | 2 | bob",
    "6 | 2 | bob | 2 | ann",
];

fn visible() -> Vec<String> {
    VISIBLE.map(str::to_owned).to_vec()
}

#[test]
fn should_show_the_edges_a_relational_join_shows_in_both_graphs() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_edges", storage);

        // Act
        let joined = join_edges(&fixture, &fixture.session);
        let first = graph_edges(&fixture, &fixture.session, "authz_a");
        let second = graph_edges(&fixture, &fixture.session, "authz_b");

        // Assert
        assert_eq!(
            (storage.name(), &joined, &first, &second),
            (storage.name(), &visible(), &visible(), &visible())
        );
    }
}

#[test]
fn should_give_every_visible_edge_the_endpoint_identities_the_join_implies() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_endpoints", storage);
        let expected = expected_identities(&join_edges(&fixture, &fixture.session));

        // Act
        let first = graph_identities(&fixture, "authz_a");
        let second = graph_identities(&fixture, "authz_b");

        // Assert
        assert_eq!(expected.len(), 20);
        assert_eq!(
            (storage.name(), &first, &second),
            (storage.name(), &expected, &expected)
        );
    }
}

#[test]
fn should_give_parallel_edges_distinct_edge_identities() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_parallel", storage);

        // Act
        let edge_keys = graph_edge_keys(&fixture, "authz_a");
        let loop_steps = fixture.rows(
            "SELECT s.vertex = t.vertex FROM GRAPH_TABLE (authz_a MATCH p = (a IS member) \
             -[e IS grants WHERE e.id = 3]-> (b IS member) COLUMNS (p AS route)) AS g \
             CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s \
             CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS t \
             WHERE s.position = 0 AND t.position = 1",
        );

        // Assert
        assert_eq!(
            (storage.name(), edge_keys),
            (
                storage.name(),
                ["1 | 1", "2 | 2", "3 | 3", "4 | 4", "6 | 6"]
                    .map(str::to_owned)
                    .to_vec()
            )
        );
        assert_eq!(loop_steps, vec![vec![cassie::types::Value::Bool(true)]]);
    }
}

#[test]
fn should_keep_identical_display_keys_of_different_tenants_apart() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_tenants", storage);

        // Act
        let tenant_one = vertex_ref(&fixture, "authz_a", "m", &member_key(1, "ann"));
        let tenant_two = vertex_ref(&fixture, "authz_a", "m", &member_key(2, "ann"));
        let labels = fixture.rows(
            "SELECT g.label FROM GRAPH_TABLE (authz_a MATCH (v IS member WHERE v.code = 'ann') \
             COLUMNS (v.label AS label)) AS g ORDER BY g.label",
        );

        // Assert
        assert_ne!(tenant_one, tenant_two);
        assert_eq!(
            (tenant_one.graph_id, tenant_one.mapping_id),
            (tenant_two.graph_id, tenant_two.mapping_id)
        );
        assert_eq!(labels.len(), 2);
    }
}

#[test]
fn should_distinguish_the_same_key_by_graph_within_one_database() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_graphs", storage);
        let key = member_key(1, "ann");

        // Act
        let first = vertex_ref(&fixture, "authz_a", "m", &key);
        let second = vertex_ref(&fixture, "authz_b", "person", &key);
        let edge_first = reference(
            &fixture.cassie,
            &fixture.session,
            ElementKind::Edge,
            "authz_a",
            "grant_edge",
            &[KeyPart::BigInt(1)],
        );
        let edge_second = reference(
            &fixture.cassie,
            &fixture.session,
            ElementKind::Edge,
            "authz_b",
            "grants_to",
            &[KeyPart::BigInt(1)],
        );

        // Assert
        assert_eq!(first.database_id, second.database_id);
        assert_ne!(first.graph_id, second.graph_id);
        assert_ne!(first.mapping_id, second.mapping_id);
        assert_ne!(first, second);
        assert_ne!(edge_first, edge_second);
        assert_eq!(first.key, second.key);
    }
}

#[test]
fn should_reject_the_reference_of_a_dangling_endpoint_edge_with_42704() {
    for storage in STORAGES {
        // Arrange
        let fixture = started_authz("graph_identity_dangling", storage);

        // Act
        let states = [5, 7].map(|id| {
            state_of(
                &fixture.cassie,
                &fixture.session,
                &format!(
                    "SELECT cassie_graph.edge_ref('authz_a', 'grant_edge', \
                     ARRAY[cassie_graph.key_part({id}::bigint)])"
                ),
            )
        });
        let missing_member = state_of(
            &fixture.cassie,
            &fixture.session,
            "SELECT cassie_graph.vertex_ref('authz_a', 'm', ARRAY[\
             cassie_graph.key_part(1::bigint), cassie_graph.key_part('zed'::text)])",
        );

        // Assert
        assert_eq!(
            (storage.name(), states, missing_member),
            (
                storage.name(),
                ["42704".to_owned(), "42704".to_owned()],
                "42704".to_owned()
            )
        );
    }
}
