use std::path::PathBuf;
use uuid::Uuid;

pub fn data_dir(name: &str) -> PathBuf {
    crate::support_temp_dirs::sweep_stale_once();
    std::env::temp_dir().join(format!("cassie-catalog-{name}-{}", Uuid::new_v4()))
}
