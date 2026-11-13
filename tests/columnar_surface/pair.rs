//! Differential fixture: the same logical data in a row table and a column table.
//!
//! Tables are created as `<name>_row` and `<name>_col`. Query templates name a
//! table as `{name}` and the fixture substitutes the mode suffix.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

pub const ROW: &str = "row";
pub const COL: &str = "col";

pub struct Pair {
    pub cassie: Cassie,
    pub session: CassieSession,
}

pub fn with_pair(test: impl FnOnce(&Pair)) {
    with_pair_config(|_| {}, test);
}

pub fn with_pair_config(
    configure: impl FnOnce(&mut CassieRuntimeConfig),
    test: impl FnOnce(&Pair),
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        configure(&mut config);
        let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
        cassie_test_support::support_sql::install_vector_extension(&cassie);
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        test(&Pair { cassie, session });
    });
    drop(runtime);
}

fn mode_suffix(mode: &str) -> &'static str {
    if mode == COL {
        " WITH (storage = column_store)"
    } else {
        ""
    }
}

fn resolve(template: &str, modes: &[(&str, &str)]) -> String {
    let mut sql = template.to_owned();
    for (name, mode) in modes {
        sql = sql.replace(&format!("{{{name}}}"), &format!("{name}_{mode}"));
    }
    sql
}

/// Every non-baseline assignment of row/column mode to `tables`.
fn mode_assignments<'a>(tables: &[&'a str]) -> Vec<Vec<(&'a str, &'static str)>> {
    let mut all = Vec::new();
    for mask in 0..(1_usize << tables.len()) {
        all.push(
            tables
                .iter()
                .enumerate()
                .map(|(index, name)| (*name, if (mask >> index) & 1 == 1 { COL } else { ROW }))
                .collect(),
        );
    }
    all
}

impl Pair {
    /// Substitutes `{table}` with `<table>_<mode>` in `template`.
    pub fn resolved(template: &str, table: &str, mode: &str) -> String {
        resolve(template, &[(table, mode)])
    }

    pub fn run(&self, sql: &str) -> Vec<Vec<Value>> {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
            .rows()
            .to_vec()
    }

    pub fn try_run(&self, sql: &str) -> Result<Vec<Vec<Value>>, CassieError> {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .map(|result| result.rows().to_vec())
    }

    /// SQLSTATE of a failing statement; panics when the statement succeeds.
    pub fn sqlstate(&self, sql: &str) -> String {
        match self.try_run(sql) {
            Ok(_) => panic!("expected failure: {sql}"),
            Err(error) => error.sql_state().to_owned(),
        }
    }

    /// Creates `<name>_row` and `<name>_col` with the same DDL and rows.
    pub fn create(&self, name: &str, columns: &str, values: &str) {
        for mode in [ROW, COL] {
            self.run(&format!(
                "CREATE TABLE {name}_{mode} ({columns}){}",
                mode_suffix(mode)
            ));
            if !values.is_empty() {
                self.run(&format!("INSERT INTO {name}_{mode} VALUES {values}"));
            }
        }
    }

    /// Runs `template` against every row/column assignment of `tables` and
    /// asserts each equals the all-row baseline. Results are compared in order
    /// when the template has an ORDER BY, otherwise as multisets.
    pub fn assert_equivalent(&self, template: &str, tables: &[&str]) {
        let ordered = template.to_ascii_uppercase().contains("ORDER BY");
        let mut assignments = mode_assignments(tables).into_iter();
        let baseline_modes = assignments.next().expect("baseline assignment");
        let baseline = self.normalized(template, &baseline_modes, ordered);
        for modes in assignments {
            let actual = self.normalized(template, &modes, ordered);
            assert_eq!(actual, baseline, "{template} with {modes:?}");
        }
    }

    fn normalized(
        &self,
        template: &str,
        modes: &[(&str, &str)],
        ordered: bool,
    ) -> Vec<Vec<Value>> {
        let mut rows = self.run(&resolve(template, modes));
        if !ordered {
            rows.sort_by_key(|row| format!("{row:?}"));
        }
        rows
    }

    pub fn sorted(&self, sql: &str) -> Vec<Vec<Value>> {
        let mut rows = self.run(sql);
        rows.sort_by_key(|row| format!("{row:?}"));
        rows
    }
}
