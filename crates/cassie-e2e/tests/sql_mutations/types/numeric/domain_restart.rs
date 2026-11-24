//! Numeric domain typmod and value identity survive a storage restart (#812).

use cassie::app::Cassie;
use cassie::types::Value;

struct NumericDomainDirectory(String);

impl Drop for NumericDomainDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
