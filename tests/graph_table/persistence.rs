//! Restart, key-part identity and insertion-order independence of `GRAPH_TABLE` reads.

use crate::support_sql::{data_dir, use_local_storage};
use crate::support_sql_fixture::sql_fixture;
use crate::world::{link, node, World};
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const ROUTES: &str = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS place WHERE s.kind = 'root') \
     -[IS road]-> (m IS place) -[IS road]-> (t IS place) \
     COLUMNS (m.kind AS mk, m.code AS mc, t.code AS tc)) AS r";

const SEPARATOR_SETUP: [&str; 5] = [
    "CREATE TABLE place (kind TEXT NOT NULL, code TEXT NOT NULL, PRIMARY KEY (kind, code))",
    "CREATE TABLE road (id TEXT PRIMARY KEY, sk TEXT NOT NULL, sc TEXT NOT NULL, \
     tk TEXT NOT NULL, tc TEXT NOT NULL)",
    "INSERT INTO place VALUES ('root', 'start'), ('a:b', 'c'), ('a', 'b:c'), ('goal', 'finish')",
    "INSERT INTO road VALUES ('dead', 'root', 'start', 'a:b', 'c'), \
     ('route', 'root', 'start', 'a', 'b:c'), ('finish', 'a', 'b:c', 'goal', 'finish')",
    "CREATE PROPERTY GRAPH g VERTEX TABLES (place AS p KEY (kind, code) LABEL place \
     PROPERTIES (kind, code)) EDGE TABLES (road AS r KEY (id) \
     SOURCE KEY (sk, sc) REFERENCES p (kind, code) \
     DESTINATION KEY (tk, tc) REFERENCES p (kind, code) LABEL road PROPERTIES (id))",
];

fn read(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("read failed with SQLSTATE {}", error.sql_state()),
    }
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn should_keep_composite_key_parts_with_separators_distinct() {
    // Arrange
    let fixture = sql_fixture("graph-separator-keys", &SEPARATOR_SETUP);

    // Act
    let routes = fixture.rows(ROUTES);

    // Assert
    assert_eq!(routes, vec![vec![text("a"), text("b:c"), text("finish")]]);
}

#[test]
fn should_return_the_same_matches_whatever_the_insertion_order() {
    // Arrange
    let nodes = (1..=5).map(node).collect::<Vec<_>>();
    let links = vec![
        link(1, 1, 2),
        link(2, 2, 3),
        link(3, 3, 1),
        link(4, 3, 4),
        link(5, 4, 5),
    ];
    let forward = World::simple(&nodes, &links);
    let reverse = World::simple(
        &nodes.into_iter().rev().collect::<Vec<_>>(),
        &links.into_iter().rev().collect::<Vec<_>>(),
    );
    let pattern = "(s IS node) -[IS link]->{1,3} (t IS node)";
    let columns = "s.num AS s, t.num AS t";

    // Act
    let from_forward = forward.query(pattern, columns);
    let from_reverse = reverse.query(pattern, columns);

    // Assert
    assert_ne!(from_forward.len(), 0);
    assert_eq!(from_forward, from_reverse);
}

#[test]
fn should_return_the_same_rows_after_a_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("graph-table-restart");
    let query = "SELECT * FROM GRAPH_TABLE (g MATCH (s IS place WHERE s.kind = 'root') \
         -[IS road]-> (m IS place) COLUMNS (m.kind AS mk, m.code AS mc)) AS r \
         ORDER BY mk, mc";
    let before = {
        let cassie = Cassie::new_with_data_dir(&path).expect("open storage");
        cassie.startup().expect("start up");
        let session = cassie.create_session("tester", None);
        for sql in SEPARATOR_SETUP {
            if let Err(error) = cassie.execute_sql(&session, sql, vec![]) {
                panic!("setup failed with SQLSTATE {}", error.sql_state());
            }
        }
        read(&cassie, &session, query)
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen storage");
    reopened.startup().expect("start up again");
    let session = reopened.create_session("tester", None);
    let after = read(&reopened, &session, query);

    // Assert
    drop(session);
    drop(reopened);
    let _ = std::fs::remove_dir_all(&path);
    assert_eq!(before.len(), 2);
    assert_eq!(before, after);
}
