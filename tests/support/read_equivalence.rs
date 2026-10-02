use cassie::app::{Cassie, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::executor::QueryResult;

pub fn with_fixture(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".into(),
            dimensions: 3,
        });
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
    });
    std::fs::remove_dir_all(path).expect("remove fixture");
}

pub fn sql(cassie: &Cassie, session: &CassieSession, statement: &str) -> QueryResult {
    cassie
        .execute_sql(session, statement, vec![])
        .expect(statement)
}
