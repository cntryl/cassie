use std::path::PathBuf;

pub fn data_dir(name: &str) -> PathBuf {
    crate::support_temp_dirs::data_path(&format!("catalog-{name}"))
}
