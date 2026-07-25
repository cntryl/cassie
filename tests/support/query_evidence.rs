use cassie::app::Cassie;
use cassie::types::Value;

use uuid::Uuid;

pub struct SeededQueryFixture {
    rows: usize,
}

pub struct PageComparison {
    pub indexed: Vec<Vec<Vec<Value>>>,
    pub row_baseline: Vec<Vec<Vec<Value>>>,
}

impl SeededQueryFixture {
    pub const fn new(rows: usize) -> Self {
        Self { rows }
    }

    pub fn compare_indexed_pages_with_overlay(&self) -> PageComparison {
        std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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

            seed(&row_cassie, false, self.rows);
            seed(&indexed_cassie, true, self.rows);

            let row_baseline = execute_pages(&row_cassie, "query_evidence_row");
            let indexed = execute_pages(&indexed_cassie, "query_evidence_indexed");

            let _ = std::fs::remove_dir_all(row_path);
            let _ = std::fs::remove_dir_all(indexed_path);

            PageComparison {
                indexed,
                row_baseline,
            }
        })
    }
}

fn seed(cassie: &Cassie, indexed: bool, rows: usize) {
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

    let values = (0..rows)
        .map(|score| {
            let category = if score.is_multiple_of(2) {
                "even"
            } else {
                "odd"
            };
            format!("('{category}', {score})")
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

fn execute_pages(cassie: &Cassie, table: &str) -> Vec<Vec<Vec<Value>>> {
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

    let pages = (0..4)
        .map(|page| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT category, score FROM {table} WHERE score >= 0 ORDER BY score DESC LIMIT 3 OFFSET {}",
                        page * 3
                    ),
                    vec![],
                )
                .expect("execute query evidence page")
                .rows
        })
        .collect::<Vec<_>>();
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback query evidence transaction");
    pages
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().into_owned()
}
