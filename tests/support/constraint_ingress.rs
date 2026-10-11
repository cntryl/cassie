use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

struct FixtureDirectory(PathBuf);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove fixture directory after owners drop");
    }
}

pub(super) struct Fixture {
    session: CassieSession,
    pub(super) cassie: Cassie,
    _directory: FixtureDirectory,
}

impl Fixture {
    pub(super) fn new() -> Self {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let directory = FixtureDirectory(
            std::env::temp_dir().join(format!("cassie-constraint-ingress-{}", Uuid::new_v4())),
        );
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("create Cassie");
        cassie.startup().expect("startup Cassie");
        let session = cassie.create_session("tester", None);
        let fixture = Self {
            session,
            cassie,
            _directory: directory,
        };
        fixture
            .execute("CREATE TABLE constraint_default_source (id INT)")
            .expect("create source table");
        fixture
            .execute("INSERT INTO constraint_default_source (id) VALUES (2)")
            .expect("seed source row");
        fixture
            .execute("CREATE TABLE constraint_default_ingress (id INT PRIMARY KEY, required TEXT NOT NULL DEFAULT 'generated', note TEXT DEFAULT 'initial')")
            .expect("create target table");
        fixture
    }

    pub(super) fn execute(&self, sql: &str) -> Result<QueryResult, CassieError> {
        self.cassie.execute_sql(&self.session, sql, vec![])
    }

    pub(super) fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        self.execute(sql).expect("select rows").rows().to_vec()
    }

    pub(super) fn copy(&self, columns: &[&str], payload: &[u8]) -> Result<usize, CassieError> {
        self.cassie.copy_from_csv_stdin(
            &self.session,
            &CopyStatement {
                table: "constraint_default_ingress".to_string(),
                columns: columns.iter().map(|column| (*column).to_string()).collect(),
                format: CopyFormat::Csv,
                header: false,
            },
            payload,
        )
    }
}
