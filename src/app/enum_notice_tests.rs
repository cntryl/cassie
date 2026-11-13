use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::sql::parse_statement;

fn fixture() -> Cassie {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    cassie
}

#[test]
fn should_notify_when_if_not_exists_names_an_existing_label() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TYPE mood AS ENUM ('sad')", vec![])
        .expect("create");
    let parsed =
        parse_statement("ALTER TYPE mood ADD VALUE IF NOT EXISTS 'sad'").expect("parse");

    // Act
    let notice = cassie.enum_add_value_skip_notice(&session, &parsed);

    // Assert
    assert_eq!(
        notice.as_deref(),
        Some("enum label \"sad\" already exists, skipping")
    );
}

#[test]
fn should_not_notify_for_a_new_label_or_without_if_not_exists() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TYPE mood AS ENUM ('sad')", vec![])
        .expect("create");
    let new_label =
        parse_statement("ALTER TYPE mood ADD VALUE IF NOT EXISTS 'happy'").expect("parse");
    let plain = parse_statement("ALTER TYPE mood ADD VALUE 'sad'").expect("parse");

    // Act
    let for_new = cassie.enum_add_value_skip_notice(&session, &new_label);
    let for_plain = cassie.enum_add_value_skip_notice(&session, &plain);

    // Assert
    assert!(for_new.is_none());
    assert!(for_plain.is_none());
}
