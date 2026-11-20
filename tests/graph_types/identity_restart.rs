//! Saved typed keys across a local restart, a supported rename, a same-key reinsertion and a
//! foreign graph, mapping or database (#1351).

use crate::identity_fixture::{
    member_key, reference, render, run_as, setup, state_of, Storage, STORAGES,
};
use crate::support_sql::{data_dir, use_local_storage};
use cassie::app::Cassie;
use cassie::types::graph::{ElementKind, ElementRef, KeyPart};
use uuid::Uuid;

struct Saved {
    vertex_a: ElementRef,
    bob_a: ElementRef,
    vertex_b: ElementRef,
    edge_a: ElementRef,
    foreign: ElementRef,
}

fn open(path: &str) -> Cassie {
    let cassie = Cassie::new_with_data_dir(path).expect("open storage");
    cassie.startup().expect("start up");
    cassie
}

/// Builds the graphs, captures references, then renames and reinserts. Returns what to
/// resolve after the restart.
fn first_run(path: &str, storage: Storage) -> Saved {
    let cassie = open(path);
    let session = cassie.create_session("tester", None);
    for sql in setup(storage) {
        run_as(&cassie, &session, &sql);
    }
    let vertex = |graph: &str, mapping: &str, key: &[KeyPart]| {
        reference(&cassie, &session, ElementKind::Vertex, graph, mapping, key)
    };
    let saved_vertex_a = vertex("authz_a", "m", &member_key(1, "ann"));
    let saved_bob_a = vertex("authz_a", "m", &member_key(1, "bob"));
    let saved_vertex_b = vertex("authz_b", "person", &member_key(1, "ann"));
    let saved_edge_a = reference(
        &cassie,
        &session,
        ElementKind::Edge,
        "authz_a",
        "grant_edge",
        &[KeyPart::BigInt(1)],
    );
    run_as(
        &cassie,
        &session,
        "ALTER PROPERTY GRAPH authz_a RENAME TO authz_a2",
    );
    run_as(
        &cassie,
        &session,
        "ALTER TABLE membership RENAME COLUMN src TO source_code",
    );
    run_as(
        &cassie,
        &session,
        "DELETE FROM member WHERE tenant_id = 1 AND code = 'bob'",
    );
    run_as(
        &cassie,
        &session,
        "INSERT INTO member VALUES (1, 'bob', 'Bob reborn')",
    );
    run_as(&cassie, &session, "CREATE DATABASE tenant_x");
    let other = cassie.create_session("tester", Some("tenant_x".to_owned()));
    for sql in setup(storage) {
        run_as(&cassie, &other, &sql);
    }
    let foreign = reference(
        &cassie,
        &other,
        ElementKind::Vertex,
        "authz_a",
        "m",
        &member_key(1, "ann"),
    );
    Saved {
        vertex_a: saved_vertex_a,
        bob_a: saved_bob_a,
        vertex_b: saved_vertex_b,
        edge_a: saved_edge_a,
        foreign,
    }
}

/// Resolutions after the restart: the saved references, then references that name another
/// database, another graph's mapping, an edge mapping as a vertex mapping or no graph.
fn outcomes(cassie: &Cassie, saved: &Saved) -> Vec<Result<(), String>> {
    let resolve = |reference: &ElementRef, database: &str| {
        cassie
            .catalog
            .resolve_element_ref(reference, database)
            .map(|_| ())
            .map_err(|error| error.sql_state().to_owned())
    };
    let variant = |change: fn(&mut ElementRef, &Saved)| {
        let mut changed = saved.vertex_a.clone();
        change(&mut changed, saved);
        changed
    };
    let cross_graph = variant(|reference, saved| reference.mapping_id = saved.vertex_b.mapping_id);
    let cross_kind = variant(|reference, saved| reference.mapping_id = saved.edge_a.mapping_id);
    let foreign_database =
        variant(|reference, saved| reference.database_id = saved.foreign.database_id);
    let no_graph = variant(|reference, _| reference.graph_id = Uuid::nil());
    vec![
        resolve(&saved.vertex_a, "postgres"),
        resolve(&saved.bob_a, "postgres"),
        resolve(&saved.vertex_b, "postgres"),
        resolve(&saved.edge_a, "postgres"),
        resolve(&saved.foreign, "tenant_x"),
        resolve(&saved.foreign, "postgres"),
        resolve(&cross_graph, "postgres"),
        resolve(&cross_kind, "postgres"),
        resolve(&foreign_database, "postgres"),
        resolve(&no_graph, "postgres"),
    ]
}

#[test]
fn should_resolve_saved_typed_keys_after_a_restart() {
    for storage in STORAGES {
        // Arrange
        use_local_storage();
        let path = data_dir("graph-identity-restart");
        let saved = first_run(&path, storage);

        // Act
        let cassie = open(&path);
        let session = cassie.create_session("tester", None);
        let renamed = reference(
            &cassie,
            &session,
            ElementKind::Vertex,
            "authz_a2",
            "m",
            &member_key(1, "ann"),
        );
        let reborn = reference(
            &cassie,
            &session,
            ElementKind::Vertex,
            "authz_a2",
            "m",
            &member_key(1, "bob"),
        );
        let label = match cassie.execute_sql(
            &session,
            "SELECT g.label FROM GRAPH_TABLE (authz_a2 MATCH (v IS member WHERE v.code = 'bob' \
             AND v.tenant_id = 1) COLUMNS (v.label AS label)) AS g",
            Vec::new(),
        ) {
            Ok(result) => render(result.rows()),
            Err(error) => vec![error.sql_state().to_owned()],
        };
        let old_name = state_of(
            &cassie,
            &session,
            "SELECT g.label FROM GRAPH_TABLE (authz_a MATCH (v IS member) \
             COLUMNS (v.label AS label)) AS g",
        );
        let outcomes = outcomes(&cassie, &saved);
        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(&path);

        // Assert
        let denied = || Err("42704".to_owned());
        assert_eq!(
            (storage.name(), outcomes),
            (
                storage.name(),
                vec![
                    Ok(()),
                    Ok(()),
                    Ok(()),
                    Ok(()),
                    Ok(()),
                    denied(),
                    denied(),
                    denied(),
                    denied(),
                    denied()
                ]
            )
        );
        assert_eq!(renamed, saved.vertex_a);
        assert_eq!(reborn, saved.bob_a);
        assert_eq!(label, vec!["Bob reborn".to_owned()]);
        assert_eq!(old_name, "42704");
    }
}
