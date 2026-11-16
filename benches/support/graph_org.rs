//! Projector-shaped authorization and hierarchy graphs over ordinary tables, with independent
//! Rust oracles. Used by the Tier 3 graph benchmark.

use super::graph_social::{Plan, TableLoad};

/// `bench_org`: a binary-heap-shaped reporting tree. Node `c > 1` reports to node `c / 2`.
pub struct Hierarchy {
    nodes: i64,
}

pub const ANCESTRY_SQL: &str = "SELECT t.ancestor FROM GRAPH_TABLE (bench_org MATCH \
     (c IS member WHERE c.id = $1) -[IS reports_to]->+ (a IS member) KEEP ALL ACYCLIC PATHS \
     COLUMNS (a.id AS ancestor)) AS t";

impl Hierarchy {
    #[must_use]
    pub const fn new(nodes: i64) -> Self {
        Self { nodes }
    }

    #[must_use]
    pub fn plan(&self) -> Plan {
        let nodes = self.nodes;
        Plan {
            tables: vec![
                "CREATE TABLE bench_org_member (id BIGINT PRIMARY KEY, name TEXT)".to_owned(),
                "CREATE TABLE bench_reports_to (id BIGINT PRIMARY KEY, child BIGINT NOT NULL, \
                 parent BIGINT NOT NULL)"
                    .to_owned(),
            ],
            loads: vec![
                TableLoad {
                    table: "bench_org_member",
                    rows: nodes,
                    row: Box::new(|key| serde_json::json!({ "id": key, "name": "member" })),
                },
                TableLoad {
                    table: "bench_reports_to",
                    rows: nodes - 1,
                    row: Box::new(|key| {
                        serde_json::json!({ "id": key, "child": key + 1, "parent": (key + 1) / 2 })
                    }),
                },
            ],
            graph: "CREATE PROPERTY GRAPH bench_org \
                    VERTEX TABLES (bench_org_member AS member KEY (id) LABEL member PROPERTIES (id, name)) \
                    EDGE TABLES (bench_reports_to AS reports_to KEY (id) \
                      SOURCE KEY (child) REFERENCES member (id) \
                      DESTINATION KEY (parent) REFERENCES member (id) \
                      LABEL reports_to PROPERTIES (id))"
                .to_owned(),
        }
    }

    /// The deepest node and its number of ancestors, which is `floor(log2(node))`.
    #[must_use]
    pub fn deepest(&self) -> (i64, usize) {
        let mut ancestors = 0;
        let mut at = self.nodes;
        while at > 1 {
            at /= 2;
            ancestors += 1;
        }
        (self.nodes, ancestors)
    }
}

/// `bench_access`: principals that belong to groups, and grants of resources to principals.
pub struct Access {
    principals: i64,
    groups: i64,
    memberships: i64,
    resources: i64,
}

pub const REACHABILITY_SQL: &str = "SELECT t.resource_id FROM GRAPH_TABLE (bench_access MATCH \
     (u IS principal WHERE u.id = $1) -[IS member_of]->{0,1} (g IS principal) \
     -[IS grants]-> (r IS resource) COLUMNS (r.id AS resource_id)) AS t";

const HOLDER_STEP: i64 = 31;

impl Access {
    /// Groups are principals `1..=groups`; each of `resources` resources is granted once.
    #[must_use]
    pub const fn new(principals: i64, groups: i64, memberships: i64, resources: i64) -> Self {
        Self {
            principals,
            groups,
            memberships,
            resources,
        }
    }

    #[must_use]
    pub fn plan(&self) -> Plan {
        let (principals, groups, memberships, resources) = (
            self.principals,
            self.groups,
            self.memberships,
            self.resources,
        );
        let users = principals - groups;
        Plan {
            tables: vec![
                "CREATE TABLE bench_principal (id BIGINT PRIMARY KEY, name TEXT)".to_owned(),
                "CREATE TABLE bench_resource (id BIGINT PRIMARY KEY, name TEXT)".to_owned(),
                "CREATE TABLE bench_member_of (id BIGINT PRIMARY KEY, member BIGINT NOT NULL, \
                 grp BIGINT NOT NULL)"
                    .to_owned(),
                "CREATE TABLE bench_grants (id BIGINT PRIMARY KEY, holder BIGINT NOT NULL, \
                 resource BIGINT NOT NULL)"
                    .to_owned(),
            ],
            loads: vec![
                TableLoad {
                    table: "bench_principal",
                    rows: principals,
                    row: Box::new(|key| serde_json::json!({ "id": key, "name": "principal" })),
                },
                TableLoad {
                    table: "bench_resource",
                    rows: resources,
                    row: Box::new(|key| serde_json::json!({ "id": key, "name": "resource" })),
                },
                TableLoad {
                    table: "bench_member_of",
                    rows: memberships,
                    row: Box::new(move |key| {
                        serde_json::json!({
                            "id": key,
                            "member": groups + (key % users) + 1,
                            "grp": key % groups + 1,
                        })
                    }),
                },
                TableLoad {
                    table: "bench_grants",
                    rows: resources,
                    row: Box::new(move |key| {
                        serde_json::json!({
                            "id": key,
                            "holder": (key * HOLDER_STEP) % principals + 1,
                            "resource": key,
                        })
                    }),
                },
            ],
            graph: "CREATE PROPERTY GRAPH bench_access \
                    VERTEX TABLES (\
                      bench_principal AS principal KEY (id) LABEL principal PROPERTIES (id, name), \
                      bench_resource AS resource KEY (id) LABEL resource PROPERTIES (id, name)) \
                    EDGE TABLES (\
                      bench_member_of AS member_of KEY (id) \
                        SOURCE KEY (member) REFERENCES principal (id) \
                        DESTINATION KEY (grp) REFERENCES principal (id) \
                        LABEL member_of PROPERTIES (id), \
                      bench_grants AS grants KEY (id) \
                        SOURCE KEY (holder) REFERENCES principal (id) \
                        DESTINATION KEY (resource) REFERENCES resource (id) \
                        LABEL grants PROPERTIES (id))"
                .to_owned(),
        }
    }

    /// The number of `(zero or one membership) then grant` walks from `user`. A repeated
    /// membership edge to one group is a separate walk.
    fn reachable(&self, user: i64) -> usize {
        let users = self.principals - self.groups;
        let held_by = |holder: i64| {
            (1..=self.resources)
                .filter(|key| (key * HOLDER_STEP) % self.principals + 1 == holder)
                .count()
        };
        let memberships = (1..=self.memberships)
            .filter(|key| self.groups + (key % users) + 1 == user)
            .map(|key| key % self.groups + 1)
            .map(held_by)
            .sum::<usize>();
        held_by(user) + memberships
    }

    /// The first user (not a group) with at least `minimum` reachable grants, and that count.
    ///
    /// # Panics
    ///
    /// Panics when no user qualifies.
    #[must_use]
    pub fn sample_user(&self, minimum: usize) -> (i64, usize) {
        (self.groups + 1..=self.principals)
            .map(|user| (user, self.reachable(user)))
            .find(|(_, count)| *count >= minimum)
            .expect("a user with reachable grants")
    }
}
