use super::support_query_evidence as query_evidence;

#[test]
fn should_compare_seeded_indexed_pages_with_overlay() {
    // Arrange
    let fixture = query_evidence::SeededQueryFixture::from_seed(0x00C4_551E, 37);
    let different_seed = query_evidence::SeededQueryFixture::from_seed(0xA11C_E002, 37);

    // Act
    let pages = fixture.compare_indexed_pages_with_overlay();
    let different_seed_pages = different_seed.compare_indexed_pages_with_overlay();

    // Assert
    assert_eq!(pages.indexed, pages.row_baseline);
    assert_eq!(pages.indexed.len(), 14);
    assert!(pages.indexed.iter().all(|page| page.len() <= 3));
    assert!(pages.indexed[0].iter().any(|row| {
        row == &vec![
            cassie::types::Value::String("overlay".to_owned()),
            cassie::types::Value::Int64(100),
        ]
    }));
    assert_eq!(pages.indexed.concat(), pages.indexed_full);
    assert_eq!(pages.row_baseline.concat(), pages.row_baseline_full);
    assert_eq!(pages.indexed_empty, pages.row_baseline_empty);
    assert_eq!(
        pages.indexed_empty,
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    assert_eq!(pages.indexed_full, pages.indexed_rewritten);
    assert_eq!(pages.row_baseline_full, pages.row_baseline_rewritten);
    assert_eq!(pages.indexed_rewritten, pages.row_baseline_rewritten);
    assert_ne!(pages.indexed_full, different_seed_pages.indexed_full);
    assert_eq!(
        different_seed_pages.indexed,
        different_seed_pages.row_baseline
    );
}
