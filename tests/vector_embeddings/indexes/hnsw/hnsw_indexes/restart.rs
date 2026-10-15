#[test]
fn should_hydrate_persisted_hnsw_graph_state_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("hnsw_graph_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_graph_restart";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);

        let stored = stored_hnsw_index(&cassie, collection);
        let graph = stored.metadata.hnsw_graph.expect("hnsw graph state");
        assert_eq!(graph.row_count, 3);
        assert_eq!(graph.dimensions, 3);
        assert_eq!(graph.metric, DistanceMetric::L2);
        assert!(graph.entry_point.is_some());
        assert_eq!(graph.nodes.len(), 3);
    }

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let stored = stored_hnsw_index(&restarted, "hnsw_graph_restart");

    // Assert
    let graph = stored
        .metadata
        .hnsw_graph
        .expect("hydrated hnsw graph state");
    assert_eq!(graph.row_count, 3);
    assert_eq!(graph.nodes.len(), 3);

    let _ = std::fs::remove_dir_all(path);
}
