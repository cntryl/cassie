use cassie::app::Cassie;
use cassie::types::Value;

use uuid::Uuid;

pub struct SeededQueryFixture {
    seed: u64,
    rows: usize,
}

pub struct PageComparison {
    pub indexed: Vec<Vec<Vec<Value>>>,
    pub row_baseline: Vec<Vec<Vec<Value>>>,
    pub indexed_full: Vec<Vec<Value>>,
    pub row_baseline_full: Vec<Vec<Value>>,
    pub indexed_empty: Vec<Vec<Value>>,
    pub row_baseline_empty: Vec<Vec<Value>>,
}

struct QueryResults {
    pages: Vec<Vec<Vec<Value>>>,
    full: Vec<Vec<Value>>,
    empty: Vec<Vec<Value>>,
}

impl SeededQueryFixture {
    pub const fn from_seed(seed: u64, rows: usize) -> Self {
        Self { seed, rows }
    }

    pub fn compare_indexed_pages_with_overlay(&self) -> PageComparison {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("query evidence runtime");

        runtime.block_on(async {
            let row_path = data_dir("query-evidence-row");
            let indexed_path = data_dir("query-evidence-indexed");
            let row_cassie = Cassie::new_with_data_dir(&row_path).expect("row Cassie");
            let indexed_cassie = Cassie::new_with_data_dir(&indexed_path).expect("indexed Cassie");
            row_cassie.startup().expect("start row Cassie");
            indexed_cassie.startup().expect("start indexed Cassie");

            let rows = seeded_rows(self.seed, self.rows);
            seed(&row_cassie, false, &rows, false);
            seed(&indexed_cassie, true, &rows, true);

            let row_baseline = execute_pages(&row_cassie, "query_evidence_row", self.rows);
            let indexed = execute_pages(&indexed_cassie, "query_evidence_indexed", self.rows);

            let _ = std::fs::remove_dir_all(row_path);
            let _ = std::fs::remove_dir_all(indexed_path);

            PageComparison {
                indexed: indexed.pages,
                row_baseline: row_baseline.pages,
                indexed_full: indexed.full,
                row_baseline_full: row_baseline.full,
                indexed_empty: indexed.empty,
                row_baseline_empty: row_baseline.empty,
            }
        })
    }
}

fn seed(cassie: &Cassie, indexed: bool, rows: &[(Option<String>, Option<i64>)], reverse: bool) {
    let session = cassie.create_session("query-evidence", None);
    let table = if indexed {
        "query_evidence_indexed"
    } else {
        "query_evidence_row"
    };
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (category TEXT, score BIGINT)"),
            vec![],
        )
        .expect("create query evidence table");

    let mut rows = rows.iter().collect::<Vec<_>>();
    if reverse {
        rows.reverse();
    }
    let values = rows
        .into_iter()
        .map(|(category, score)| {
            let category = category
                .as_ref()
                .map_or_else(|| "NULL".to_owned(), |value| format!("'{value}'"));
            let score = score.map_or_else(|| "NULL".to_owned(), |value| value.to_string());
            format!("({category}, {score})")
        })
        .collect::<Vec<_>>()
        .join(", ");
    cassie
        .execute_sql(
            &session,
            &format!("INSERT INTO {table} (category, score) VALUES {values}"),
            vec![],
        )
        .expect("seed query evidence rows");
    if indexed {
        cassie
            .execute_sql(
                &session,
                &format!("CREATE INDEX {table}_score_idx ON {table} USING btree (score)"),
                vec![],
            )
            .expect("create query evidence index");
    }
}

fn execute_pages(cassie: &Cassie, table: &str, row_count: usize) -> QueryResults {
    let session = cassie.create_session("query-evidence", None);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin query evidence transaction");
    cassie
        .execute_sql(
            &session,
            &format!("INSERT INTO {table} (category, score) VALUES ('overlay', 100)"),
            vec![],
        )
        .expect("insert query evidence overlay");

    let pages = (0..=row_count.div_ceil(3))
        .map(|page| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT category, score FROM {table} WHERE score >= 0 ORDER BY score DESC, category ASC LIMIT 3 OFFSET {}",
                        page * 3
                    ),
                    vec![],
                )
                .expect("execute query evidence page")
                .rows
        })
        .collect::<Vec<_>>();
    let full = cassie
        .execute_sql(
            &session,
            &format!(
                "SELECT category, score FROM {table} WHERE score >= 0 ORDER BY score DESC, category ASC"
            ),
            vec![],
        )
        .expect("execute full query evidence result")
        .rows;
    let empty = cassie
        .execute_sql(
            &session,
            &format!(
                "SELECT category, score FROM {table} WHERE score >= 1000 ORDER BY score DESC, category ASC"
            ),
            vec![],
        )
        .expect("execute empty query evidence result")
        .rows;
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback query evidence transaction");
    QueryResults { pages, full, empty }
}

fn seeded_rows(seed: u64, rows: usize) -> Vec<(Option<String>, Option<i64>)> {
    let mut state = seed;
    (0..rows)
        .map(|index| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut mixed = state;
            mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            mixed ^= mixed >> 31;

            match index {
                0 => (None, Some(0)),
                1 | 2 => (Some("tie".to_owned()), Some(7)),
                3 => (Some("null-score".to_owned()), None),
                _ => {
                    let category = Some(format!("group-{}", mixed % 5));
                    let score = i64::try_from(mixed % 41).expect("bounded score") - 20;
                    (category, Some(score))
                }
            }
        })
        .collect()
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().into_owned()
}
