//! Thread-local observations at graph allocation boundaries; absent from production builds.

use std::cell::RefCell;

thread_local! {
    static EVENTS: RefCell<Option<Vec<(&'static str, usize)>>> = const { RefCell::new(None) };
    static OWNER: RefCell<Option<crate::runtime::QueryExecutionControls>> = const { RefCell::new(None) };
}

pub(crate) struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        EVENTS.with(|events| {
            events.borrow_mut().take();
        });
        OWNER.with(|owner| {
            owner.borrow_mut().take();
        });
    }
}

pub(crate) fn start() -> Guard {
    EVENTS.with(|events| {
        assert!(
            events.borrow().is_none(),
            "graph memory observations cannot nest"
        );
        events.replace(Some(Vec::new()));
    });
    Guard
}

pub(crate) fn record(boundary: &'static str, bytes: usize) {
    EVENTS.with(|events| {
        if let Some(events) = events.borrow_mut().as_mut() {
            events.push((boundary, bytes));
        }
    });
}

pub(crate) fn events() -> Vec<(&'static str, usize)> {
    EVENTS.with(|events| events.borrow().clone().expect("active observations"))
}

pub(crate) fn expect_owner(controls: &crate::runtime::QueryExecutionControls) {
    OWNER.with(|owner| owner.replace(Some(controls.clone())));
}

pub(crate) fn record_owner(
    boundary: &'static str,
    controls: &crate::runtime::QueryExecutionControls,
) {
    OWNER.with(|owner| {
        if let Some(expected) = owner.borrow().as_ref() {
            let lease = controls
                .reserve_query_memory(0)
                .expect("zero-byte observer lease");
            record(boundary, usize::from(lease.belongs_to(expected)));
        }
    });
}

pub(crate) fn record_current(boundary: &'static str) {
    OWNER.with(|owner| {
        if let Some(controls) = owner.borrow().as_ref() {
            record(boundary, controls.current_query_memory_bytes());
        }
    });
}

pub(crate) fn assert_admitted(before: &'static str, copy: &'static str, required: usize) {
    let events = events();
    let mut base = None;
    let mut copies = 0;
    for (boundary, bytes) in events {
        if boundary == before {
            base = Some(bytes);
        } else if boundary == copy {
            let base = base.expect("copy has an immediate pre-admission boundary");
            assert!(
                bytes >= base + required,
                "{copy} copied before admission: base={base}, at_copy={bytes}, required={required}"
            );
            copies += 1;
        }
    }
    assert!(copies > 0, "the tiny fixture must reach {copy}");
}

pub(crate) fn fixture() -> (crate::app::Cassie, crate::app::CassieSession) {
    let mut config = crate::config::CassieRuntimeConfig::from_env().expect("configuration");
    config.password = uuid::Uuid::new_v4().to_string();
    let cassie = crate::app::Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE v(id INT PRIMARY KEY)",
        "CREATE TABLE e(id INT PRIMARY KEY,s INT,t INT)",
        "INSERT INTO v VALUES(1),(2)",
        "INSERT INTO e VALUES(1,1,2)",
        "CREATE INDEX e_s ON e(s)",
        "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
         EDGE TABLES(e AS l KEY(id) SOURCE KEY(s) REFERENCES n(id) \
         DESTINATION KEY(t) REFERENCES n(id) LABEL edge PROPERTIES(id))",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("seed tiny graph");
    }
    (cassie, session)
}
