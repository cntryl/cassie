use uuid::Uuid;

pub fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

pub fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}
