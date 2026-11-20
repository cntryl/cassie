//! Shared fixture of the stable-identity qualification (#1351): one tenant-qualified
//! composite-key projection mapped into two named graphs under two aliases, over row or
//! column-authoritative sources.

use crate::fixture::{started, GraphFixture};
use cassie::app::{Cassie, CassieSession};
use cassie::types::graph::{ElementKind, ElementRef, KeyPart};
use cassie::types::Value;

#[derive(Clone, Copy, Debug)]
pub enum Storage {
    Row,
    Columnar,
}

pub const STORAGES: [Storage; 2] = [Storage::Row, Storage::Columnar];

impl Storage {
    fn clause(self) -> &'static str {
        match self {
            Self::Row => "",
            Self::Columnar => " USING columnar",
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Row => "row",
            Self::Columnar => "columnar",
        }
    }
}

/// The fixture statements, in order. Tenants 1 and 2 both hold members `ann` and `bob`.
/// Membership 2 parallels 1, 3 is a self-loop, 5 points at a missing member of tenant 1 and
/// 7 at a missing member of tenant 2.
#[must_use]
pub fn setup(storage: Storage) -> Vec<String> {
    let using = storage.clause();
    vec![
        format!(
            "CREATE TABLE member (tenant_id BIGINT NOT NULL, code TEXT NOT NULL, label TEXT, \
             PRIMARY KEY (tenant_id, code)){using}"
        ),
        format!(
            "CREATE TABLE membership (id BIGINT PRIMARY KEY, tenant_id BIGINT NOT NULL, \
             src TEXT NOT NULL, dst TEXT NOT NULL){using}"
        ),
        "INSERT INTO member VALUES (1, 'ann', 'Ann one'), (1, 'bob', 'Bob one'), \
         (2, 'ann', 'Ann two'), (2, 'bob', 'Bob two')"
            .to_owned(),
        "INSERT INTO membership VALUES (1, 1, 'ann', 'bob'), (2, 1, 'ann', 'bob'), \
         (3, 1, 'bob', 'bob'), (4, 2, 'ann', 'bob'), (5, 1, 'ann', 'zed'), \
         (6, 2, 'bob', 'ann'), (7, 2, 'bob', 'cy')"
            .to_owned(),
        graph("authz_a", "m", "grant_edge"),
        graph("authz_b", "person", "grants_to"),
    ]
}

fn graph(name: &str, vertex: &str, edge: &str) -> String {
    format!(
        "CREATE PROPERTY GRAPH {name} \
         VERTEX TABLES (member AS {vertex} KEY (tenant_id, code) LABEL member \
         PROPERTIES (tenant_id, code, label)) \
         EDGE TABLES (membership AS {edge} KEY (id) \
         SOURCE KEY (tenant_id, src) REFERENCES {vertex} (tenant_id, code) \
         DESTINATION KEY (tenant_id, dst) REFERENCES {vertex} (tenant_id, code) \
         LABEL grants PROPERTIES (id))"
    )
}

#[must_use]
pub fn started_authz(label: &str, storage: Storage) -> GraphFixture {
    let statements = setup(storage);
    let refs = statements.iter().map(String::as_str).collect::<Vec<_>>();
    started(label, &refs)
}

fn rows_as(fixture: &GraphFixture, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match fixture.cassie.execute_sql(session, sql, Vec::new()) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("query failed with SQLSTATE {}", error.sql_state()),
    }
}

/// The visible edges of `graph` as `id | source tenant | source code | target tenant |
/// target code` lines.
#[must_use]
pub fn graph_edges(fixture: &GraphFixture, session: &CassieSession, graph: &str) -> Vec<String> {
    let sql = format!(
        "SELECT g.id, g.st, g.sc, g.dt, g.dc FROM GRAPH_TABLE ({graph} \
         MATCH (s IS member) -[e IS grants]-> (d IS member) \
         COLUMNS (e.id AS id, s.tenant_id AS st, s.code AS sc, d.tenant_id AS dt, d.code AS dc)) \
         AS g ORDER BY g.id"
    );
    render(&rows_as(fixture, session, &sql))
}

/// The same edges by a plain relational join, with no graph machinery.
#[must_use]
pub fn join_edges(fixture: &GraphFixture, session: &CassieSession) -> Vec<String> {
    let sql = "SELECT l.id, s.tenant_id, s.code, d.tenant_id, d.code FROM membership AS l \
         JOIN member AS s ON s.tenant_id = l.tenant_id AND s.code = l.src \
         JOIN member AS d ON d.tenant_id = l.tenant_id AND d.code = l.dst \
         ORDER BY l.id";
    render(&rows_as(fixture, session, sql))
}

/// The element identities of `graph`'s visible edges, one line per step key component:
/// `edge id | step position | key ordinal | integer value | text value`.
#[must_use]
pub fn graph_identities(fixture: &GraphFixture, graph: &str) -> Vec<String> {
    let sql = format!(
        "SELECT g.id, s.position, k.ordinal, k.integer_value, k.text_value FROM GRAPH_TABLE \
         ({graph} MATCH p = (s IS member) -[e IS grants]-> (d IS member) \
         COLUMNS (e.id AS id, p AS route)) AS g \
         CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s \
         CROSS JOIN LATERAL cassie_graph.key_parts(s.vertex) AS k \
         ORDER BY g.id, s.position, k.ordinal"
    );
    render(&rows_as(fixture, &fixture.session, &sql))
}

/// The identities the relational join implies for the same edges.
#[must_use]
pub fn expected_identities(joined: &[String]) -> Vec<String> {
    joined
        .iter()
        .flat_map(|line| {
            let cells = line.split(" | ").collect::<Vec<_>>();
            [
                format!("{} | 0 | 1 | {} | NULL", cells[0], cells[1]),
                format!("{} | 0 | 2 | NULL | {}", cells[0], cells[2]),
                format!("{} | 1 | 1 | {} | NULL", cells[0], cells[3]),
                format!("{} | 1 | 2 | NULL | {}", cells[0], cells[4]),
            ]
        })
        .collect()
}

/// The edge identity at the last step of every visible path: `edge id | edge key`.
#[must_use]
pub fn graph_edge_keys(fixture: &GraphFixture, graph: &str) -> Vec<String> {
    let sql = format!(
        "SELECT g.id, k.integer_value FROM GRAPH_TABLE \
         ({graph} MATCH p = (s IS member) -[e IS grants]-> (d IS member) \
         COLUMNS (e.id AS id, p AS route)) AS g \
         CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s \
         CROSS JOIN LATERAL cassie_graph.key_parts(s.edge) AS k \
         WHERE s.position = 1 ORDER BY g.id"
    );
    render(&rows_as(fixture, &fixture.session, &sql))
}

/// Runs `sql` in `session` and returns its failing SQLSTATE.
///
/// # Panics
///
/// Panics when the statement succeeds.
#[must_use]
pub fn state_of(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
    match cassie.execute_sql(session, sql, Vec::new()) {
        Ok(_) => panic!("a statement that should fail succeeded"),
        Err(error) => error.sql_state().to_owned(),
    }
}

/// Runs a statement that must succeed.
///
/// # Panics
///
/// Panics when the statement fails.
pub fn run_as(cassie: &Cassie, session: &CassieSession, sql: &str) {
    if let Err(error) = cassie.execute_sql(session, sql, Vec::new()) {
        panic!("statement failed with SQLSTATE {}", error.sql_state());
    }
}

/// Renders integer, text and NULL cells joined by ` | `.
///
/// # Panics
///
/// Panics on any other value type.
#[must_use]
pub fn render(rows: &[Vec<Value>]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    Value::Null => "NULL".to_owned(),
                    Value::Int64(number) => number.to_string(),
                    Value::String(text) => text.clone(),
                    _ => panic!("unexpected value type in an identity result"),
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect()
}

/// The `key_part` array expression of `parts`.
///
/// # Panics
///
/// Panics on a key type the fixture does not use.
#[must_use]
pub fn key_array(parts: &[KeyPart]) -> String {
    let items = parts
        .iter()
        .map(|part| match part {
            KeyPart::BigInt(number) => format!("cassie_graph.key_part({number}::bigint)"),
            KeyPart::Text(text) => format!("cassie_graph.key_part('{text}'::text)"),
            _ => panic!("the fixture uses bigint and text keys only"),
        })
        .collect::<Vec<_>>();
    format!("ARRAY[{}]", items.join(", "))
}

#[must_use]
pub fn member_key(tenant: i64, code: &str) -> Vec<KeyPart> {
    vec![KeyPart::BigInt(tenant), KeyPart::Text(code.to_owned())]
}

/// The constructor expression of one element.
#[must_use]
pub fn ref_sql(kind: ElementKind, graph: &str, mapping: &str, key: &[KeyPart]) -> String {
    let function = match kind {
        ElementKind::Vertex => "vertex_ref",
        ElementKind::Edge => "edge_ref",
    };
    format!(
        "cassie_graph.{function}('{graph}', '{mapping}', {})",
        key_array(key)
    )
}

/// The durable reference the constructor yields, rebuilt from its record fields.
///
/// # Panics
///
/// Panics when the constructor fails.
#[must_use]
pub fn reference(
    cassie: &Cassie,
    session: &CassieSession,
    kind: ElementKind,
    graph: &str,
    mapping: &str,
    key: &[KeyPart],
) -> ElementRef {
    let expression = ref_sql(kind, graph, mapping, key);
    let sql = format!(
        "SELECT (r).database_id, (r).graph_id, (r).mapping_id, (r).element_kind \
         FROM (SELECT {expression} AS r) AS t"
    );
    let rows = match cassie.execute_sql(session, &sql, Vec::new()) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("constructor failed with SQLSTATE {}", error.sql_state()),
    };
    let id = |value: &Value| match value {
        Value::String(text) => uuid::Uuid::parse_str(text).expect("uuid"),
        _ => panic!("expected uuid text"),
    };
    ElementRef {
        database_id: id(&rows[0][0]),
        graph_id: id(&rows[0][1]),
        mapping_id: id(&rows[0][2]),
        kind,
        key: key.to_vec(),
    }
}

#[must_use]
pub fn vertex_ref(
    fixture: &GraphFixture,
    graph: &str,
    mapping: &str,
    key: &[KeyPart],
) -> ElementRef {
    reference(
        &fixture.cassie,
        &fixture.session,
        ElementKind::Vertex,
        graph,
        mapping,
        key,
    )
}
