use std::future::{ready, Ready};
use std::sync::Arc;

use cassie_engine::app::{Cassie, CassieError, CassieSession};
use cassie_engine::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie_engine::types::{DataType, FieldSchema, Schema};

pub struct BenchContext {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
}

pub fn unindexed_context(
    _label: &str,
    _dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    let result = (|| {
        let mut config = CassieRuntimeConfig::from_env()?;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Arc::new(Cassie::new_in_memory_with_config(config)?);
        cassie.startup()?;
        let session = cassie.create_session("benchmark", None);
        Ok(BenchContext { cassie, session })
    })();
    ready(result)
}

pub(super) fn bench_document_schema() -> Schema {
    Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_owned(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "title".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_owned(),
                data_type: DataType::Int,
                nullable: true,
            },
            FieldSchema {
                name: "status".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "embedding".to_owned(),
                data_type: DataType::Vector(Some(3)),
                nullable: true,
            },
        ],
    }
}
