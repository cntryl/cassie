//! The documented total order of tied selector choices survives restarts and does not
//! depend on insertion order (#1257).

use crate::support_sql::{data_dir, use_local_storage};
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const SCHEMA: [&str; 2] = [
    "CREATE TABLE node (num BIGINT PRIMARY KEY)",
    "CREATE TABLE link (num BIGINT PRIMARY KEY, src BIGINT NOT NULL, dst BIGINT NOT NULL)",
];

const GRAPH: &str = "CREATE PROPERTY GRAPH g \
     VERTEX TABLES (node AS n KEY (num) LABEL node PROPERTIES (num)) \
     EDGE TABLES (link AS l KEY (num) SOURCE KEY (src) REFERENCES n (num) \
     DESTINATION KEY (dst) REFERENCES n (num) LABEL link PROPERTIES (num))";

/// Two tied two-hop routes 1-2-4 and 1-3-4, with parallel edges 6 and 7 between 1 and 2.
const LINKS: [(i64, i64, i64); 5] = [(9, 1, 3), (8, 3, 4), (7, 1, 2), (5, 2, 4), (6, 1, 2)];

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn populate(cassie: &Cassie, session: &CassieSession, reverse: bool) {
    for sql in SCHEMA {
        run(cassie, session, sql);
    }
    let mut nodes = vec![1_i64, 2, 3, 4];
    let mut links = LINKS.to_vec();
    if reverse {
        nodes.reverse();
        links.reverse();
    }
    for num in nodes {
        run(cassie, session, &format!("INSERT INTO node VALUES ({num})"));
    }
    for (num, src, dst) in links {
        run(
            cassie,
            session,
            &format!("INSERT INTO link VALUES ({num}, {src}, {dst})"),
        );
    }
    run(cassie, session, GRAPH);
}

/// Element numbers (vertices then edges) of each selected path, in result order.
fn choices(cassie: &Cassie, session: &CassieSession, selector: &str) -> Vec<Vec<String>> {
    let sql = format!(
        "SELECT cassie_graph.path_to_jsonb(g.path) FROM GRAPH_TABLE (g MATCH \
         p = (s IS node WHERE s.num = 1) -[e IS link]->{{1,2}} (t IS node WHERE t.num = 4) \
         {selector} COLUMNS (p AS path)) AS g"
    );
    run(cassie, session, &sql)
        .iter()
        .map(|row| {
            let Value::Json(document) = &row[0] else {
                panic!("path_to_jsonb returns jsonb");
            };
            ["vertices", "edges"]
                .iter()
                .flat_map(|field| {
                    document[*field]
                        .as_array()
                        .expect("an element array")
                        .iter()
                        .map(|element| {
                            element["key_parts"][0]["value"]
                                .as_str()
                                .expect("a key value")
                                .to_owned()
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect()
}

fn both(cassie: &Cassie, session: &CassieSession) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    (
        choices(cassie, session, "KEEP ANY SHORTEST PATH"),
        choices(cassie, session, "KEEP SHORTEST 2 PATHS"),
    )
}

fn tied_choices(label: &str, reverse: bool, restart: bool) -> (Vec<Vec<String>>, Vec<Vec<String>>) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("open storage");
    cassie.startup().expect("start up");
    let session = cassie.create_session("tester", None);
    populate(&cassie, &session, reverse);
    let before = both(&cassie, &session);
    drop(session);
    drop(cassie);
    let after = if restart {
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen storage");
        reopened.startup().expect("start up again");
        let session = reopened.create_session("tester", None);
        both(&reopened, &session)
    } else {
        before.clone()
    };
    let _ = std::fs::remove_dir_all(&path);
    assert_eq!(before, after);
    before
}

fn strings(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|part| (*part).to_owned()).collect())
        .collect()
}

#[test]
fn should_repeat_tied_selector_choices_after_a_restart() {
    // Arrange
    let expected_any = strings(&[&["1", "2", "4", "6", "5"]]);
    let expected_two = strings(&[&["1", "2", "4", "6", "5"], &["1", "2", "4", "7", "5"]]);

    // Act
    let (any, two) = tied_choices("graph-tie-restart", false, true);

    // Assert
    assert_eq!(any, expected_any);
    assert_eq!(two, expected_two);
}

#[test]
fn should_choose_the_same_tied_paths_whatever_the_insertion_order() {
    // Arrange
    let forward = tied_choices("graph-tie-forward", false, false);

    // Act
    let reversed = tied_choices("graph-tie-reverse", true, true);

    // Assert
    assert_eq!(forward, reversed);
}
