//! PostgreSQL 18 statistics import/reset functions emitted by `pg_dump`.

use crate::support_pgwire as wire;
use crate::support_wire_queries as fixture;

const RELATION_RESTORE: &str = "SELECT pg_restore_relation_stats(\
    'schemaname', 'public', 'relname', 'stats_restore_rows', \
    'relpages', 173::integer, 'reltuples', 10000::real, \
    'relallvisible', 12::integer, 'relallfrozen', 7::integer)";
const RELATION_READ: &str = "SELECT relpages, reltuples, relallvisible, relallfrozen \
    FROM pg_catalog.pg_class WHERE oid = 'stats_restore_rows'::regclass";
const ATTRIBUTE_RESTORE: &str = "SELECT pg_restore_attribute_stats(\
    'schemaname', 'public', 'relname', 'stats_restore_rows', \
    'attname', 'k', 'inherited', false, 'avg_width', 125::integer, \
    'null_frac', 0.5::real, 'n_distinct', 17::real)";
const ATTRIBUTE_READ: &str = "SELECT attname, avg_width, null_frac, n_distinct \
    FROM pg_catalog.pg_stats WHERE schemaname = 'public' AND tablename = 'stats_restore_rows'";

fn run(label: &str, statements: &[&str]) -> Vec<fixture::Frames> {
    fixture::run_cycles(
        label,
        &[("CREATE TABLE stats_restore_rows (k integer)", vec![])],
        statements
            .iter()
            .map(|sql| vec![wire::simple_query_frame(sql)])
            .collect(),
    )
}

fn assert_row(frames: &fixture::Frames, expected_oids: &[i32], expected: &[&str]) {
    fixture::assert_success(frames);
    let (_, body) = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(body)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![expected
            .iter()
            .map(|value| Some((*value).to_owned()))
            .collect::<Vec<_>>()]
    );
}
