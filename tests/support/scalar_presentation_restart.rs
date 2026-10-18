//! Actual persisted SQL/view rebinding for source-aware scalar casts.
use cassie::app::Cassie;
use cassie::executor::QueryResult;
use cassie::types::Value;

struct Directory(String);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            assert!(
                std::thread::panicking(),
                "unable to remove presentation fixture: {error}"
            );
        }
    }
}

pub fn restart_outputs() -> (QueryResult, QueryResult) {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("native-presentation-rebind");
    let _directory = Directory(path.clone());
    let query = "SELECT temporal, floating FROM presentation_view";
    let before = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create presentation fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE presentation_table(v TIMESTAMP, f FLOAT8)",
            "CREATE VIEW presentation_view AS SELECT v::TEXT AS temporal, f::TEXT AS floating FROM presentation_table",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect("seed typed view");
        }
        cassie
            .execute_sql(
                &session,
                "INSERT INTO presentation_table VALUES('2024-02-29 12:34:56.120000', $1)",
                vec![Value::Float64(1e20)],
            )
            .expect("seed declared FLOAT8 value");
        let result = cassie
            .execute_sql(&session, query, vec![])
            .expect("first view binding");
        drop(session);
        cassie.shutdown();
        result
    };
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen presentation fixture");
    restarted.startup().expect("restart");
    let session = restarted.create_session("tester", None);
    let after = restarted
        .execute_sql(&session, query, vec![])
        .expect("rebound view");
    drop(session);
    restarted.shutdown();
    (before, after)
}
