//! SQL index publication around an independently prepared journal.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::catalog::{IndexKind, IndexMeta};
use crate::midge::Midge;

#[test]
fn should_publish_target_index_when_unrelated_journal_appears_after_the_check() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE target_rows (id INT)", vec![])
        .expect("target");
    cassie
        .execute_sql(&session, "CREATE TABLE other_rows (id INT)", vec![])
        .expect("other");
    let other = IndexMeta {
        definition_semantics_version: 0,
        collection: "other_rows".to_owned(),
        name: "other_idx".to_owned(),
        field: "id".to_owned(),
        fields: vec!["id".to_owned()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    };
    let midge = Arc::clone(&cassie.midge);
    Midge::observe_next_index_journal_check_for_test(move || {
        std::thread::spawn(move || {
            cassie_storage::adapter::set_index_publication_failure_point(true);
            let result = midge.put_index(&other);
            cassie_storage::adapter::set_index_publication_failure_point(false);
            assert!(result.is_err(), "leave an unrelated prepared journal");
        })
        .join()
        .expect("journal worker");
    });

    // Act
    let target = cassie.execute_sql(
        &session,
        "CREATE INDEX target_idx ON target_rows (id)",
        vec![],
    );

    // Assert
    target.expect("target publication remains successful");
    assert!(cassie
        .catalog
        .get_index("target_rows", "target_idx")
        .is_some());
    assert!(cassie
        .midge
        .get_index("target_rows", "target_idx")
        .expect("durable target")
        .is_some());
    assert!(cassie
        .midge
        .has_prepared_index_publication_for_test("other_rows", "other_idx")
        .expect("journal remains"));
    cassie
        .midge
        .replay_pending_index_publications()
        .expect("explicit recovery");
    assert!(!cassie
        .midge
        .has_prepared_index_publication_for_test("other_rows", "other_idx")
        .expect("journal removed"));
}
