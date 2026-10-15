use super::*;
use cassie::types::Value;

#[test]
fn should_preserve_the_instant_given_a_named_zone_input_when_storage_restarts() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("named_zone_instant_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE compat_named_zone_restart (id INTEGER PRIMARY KEY, occurred_at TIMESTAMPTZ)",
            vec![],
        )
        .expect("create a table for the stored named-zone instant");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO compat_named_zone_restart VALUES \
             (1, TIMESTAMPTZ '2024-03-10 01:30:00 America/New_York')",
            vec![],
        )
        .expect("store the instant before the New York spring DST transition");
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted.startup().expect("recover stored timestamp data");
    let session = restarted.create_session("tester", None);
    let epoch = restarted
        .execute_sql(
            &session,
            "SELECT EXTRACT(EPOCH FROM occurred_at)::BIGINT \
             FROM compat_named_zone_restart WHERE id = 1",
            vec![],
        )
        .expect("read the stored instant after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(epoch, vec![vec![Value::Int64(1_710_052_200)]]);
}
