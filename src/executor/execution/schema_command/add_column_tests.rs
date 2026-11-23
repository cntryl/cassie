use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use std::sync::mpsc;
use std::time::Duration;

type BeforeGateObserver = (mpsc::Sender<()>, mpsc::Receiver<()>);

thread_local! {
    static BEFORE_ADD_COLUMN_GATE_OBSERVER: std::cell::RefCell<Option<BeforeGateObserver>> =
        const { std::cell::RefCell::new(None) };
}

const TABLE: &str = "add_column_constraint_race";

pub(super) fn observe_before_add_column_gate() {
    let observer = BEFORE_ADD_COLUMN_GATE_OBSERVER.with(|observer| observer.borrow_mut().take());
    if let Some((entered, resume)) = observer {
        entered.send(()).expect("signal ADD COLUMN gate observer");
        resume
            .recv_timeout(Duration::from_secs(10))
            .expect("release ADD COLUMN gate observer");
    }
}

fn install_before_add_column_gate_observer(entered: mpsc::Sender<()>, resume: mpsc::Receiver<()>) {
    BEFORE_ADD_COLUMN_GATE_OBSERVER.with(|observer| {
        *observer.borrow_mut() = Some((entered, resume));
    });
}

#[test]
fn should_preserve_constraints_published_before_gated_add_column() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("create in-memory Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE add_column_constraint_race (id INT)",
            vec![],
        )
        .expect("create table");
    let (entered_tx, entered_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let plain_column_cassie = cassie.clone();
    let plain_column = std::thread::spawn(move || {
        let session = plain_column_cassie.create_session("tester", None);
        install_before_add_column_gate_observer(entered_tx, resume_rx);
        plain_column_cassie.execute_sql(
            &session,
            "ALTER TABLE add_column_constraint_race ADD COLUMN plain_value INT",
            vec![],
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("plain ADD COLUMN reached gate boundary");

    // Act
    let constrained_result = cassie.execute_sql(
        &session,
        "ALTER TABLE add_column_constraint_race ADD COLUMN required_value INT NOT NULL",
        vec![],
    );
    resume_tx.send(()).expect("release plain ADD COLUMN");
    let plain_result = plain_column.join().expect("plain ADD COLUMN thread");

    // Assert
    constrained_result.expect("publish constrained column");
    plain_result.expect("publish plain column after constrained column");
    let catalog_schema = cassie
        .catalog
        .get_schema(TABLE)
        .expect("catalog schema after both writes");
    let storage_schema = cassie
        .midge
        .collection_schema(TABLE)
        .expect("durable schema after both writes");
    assert!(catalog_schema
        .fields
        .iter()
        .any(|field| field.name == "plain_value"));
    assert!(catalog_schema
        .fields
        .iter()
        .any(|field| field.name == "required_value"));
    assert!(storage_schema
        .fields
        .iter()
        .any(|field| field.name == "plain_value"));
    assert!(storage_schema
        .fields
        .iter()
        .any(|field| field.name == "required_value"));

    let catalog_constraints = cassie.catalog.get_constraints(TABLE);
    let durable_constraints = cassie
        .midge
        .load_constraints(TABLE)
        .expect("durable constraint metadata");
    assert!(catalog_constraints
        .iter()
        .any(|constraint| { constraint.field == "required_value" && constraint.not_null }));
    assert!(durable_constraints
        .iter()
        .any(|constraint| { constraint.field == "required_value" && constraint.not_null }));
}
