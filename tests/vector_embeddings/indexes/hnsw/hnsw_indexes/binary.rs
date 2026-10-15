#[test]
fn should_store_hnsw_nodes_as_point_readable_binary_records() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_point_nodes");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (_, entries, _, node_count, monolithic_graph_count) =
        inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert_eq!(node_count, 2);
    assert_eq!(monolithic_graph_count, 0);
    assert!(entries.iter().all(|(_, raw)| raw.first() != Some(&0x7b)));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_hnsw_manifest_as_binary_record() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_binary_manifest");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (_, _, state_value, _, _) = inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert_eq!(state_value.first(), Some(&3));
    assert_ne!(state_value.first(), Some(&0x7b));

    let _ = std::fs::remove_dir_all(path);
}
