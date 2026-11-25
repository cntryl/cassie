pub fn data_dir(label: &str) -> String {
    crate::support_temp_dirs::data_path(&format!("metrics-{label}"))
        .to_string_lossy()
        .into_owned()
}
