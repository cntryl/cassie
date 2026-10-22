use cassie::app::{Cassie, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::executor::QueryResult;

pub fn with_fixture(_label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    with_fixture_config("", |_| {}, test);
}

pub fn with_fixture_config(
    _label: &str,
    configure: impl FnOnce(&mut CassieRuntimeConfig),
    test: impl FnOnce(&Cassie, &CassieSession),
) {
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
        configure(&mut config);
        let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
    });
    drop(runtime);
}

pub fn sql(cassie: &Cassie, session: &CassieSession, statement: &str) -> QueryResult {
    cassie
        .execute_sql(session, statement, vec![])
        .expect(statement)
}
