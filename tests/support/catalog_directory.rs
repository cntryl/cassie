//! Owned local-storage directory for catalog restart probes.
use std::path::PathBuf;

pub struct CatalogDirectory(pub PathBuf);

impl CatalogDirectory {
    pub fn new(label: &str) -> Self {
        let path = crate::support_data_dir::data_dir(label);
        std::fs::create_dir_all(&path).expect("catalog directory");
        Self(path)
    }
}

impl Drop for CatalogDirectory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("catalog directory cleanup failed: {error}");
            } else {
                panic!("catalog directory cleanup failed: {error}");
            }
        }
    }
}
