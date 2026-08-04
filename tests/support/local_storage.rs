pub fn use_local_storage() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}
