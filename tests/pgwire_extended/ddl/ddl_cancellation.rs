//! Canceled selected DDL reports 57014 and leaves a clean savepoint or statement recovery.

use crate::support_pgwire as wire;
use crate::support_pgwire_cancel_session::{ready_status, scalar, CancelSession, Frames};

struct Form {
    name: &'static str,
    setup: &'static [&'static str],
    doomed: &'static str,
    probe: &'static str,
    before: &'static str,
    retry: &'static [&'static str],
    after: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wire {
    Simple,
    Extended,
}

fn doomed_frames(wire_kind: Wire, sql: &str) -> Vec<Vec<u8>> {
    match wire_kind {
        Wire::Simple => vec![wire::simple_query_frame(sql)],
        Wire::Extended => vec![wire::execute_frame("doomed_ddl"), wire::sync_frame()],
    }
}

fn assert_canceled(label: &str, frames: &Frames, status: u8) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("57014"),
        "{label}: cancellation SQLSTATE"
    );
    assert_eq!(ready_status(frames), Some(status), "{label}: ready status");
    assert!(
        !frames.iter().any(|(tag, _)| *tag == b'C'),
        "{label}: no CommandComplete for a canceled statement"
    );
}

fn assert_clean(label: &str, frames: &Frames, status: u8) {
    assert_eq!(wire::error_code(frames), None, "{label}: unexpected error");
    assert_eq!(ready_status(frames), Some(status), "{label}: ready status");
}

async fn run_form(form: &Form, explicit: bool, wire_kind: Wire) {
    let label = format!(
        "{} ({}, {})",
        form.name,
        if explicit { "explicit" } else { "implicit" },
        if wire_kind == Wire::Simple {
            "simple"
        } else {
            "extended"
        }
    );
    let mut session = CancelSession::start().await;
    for sql in form.setup {
        assert_clean(&label, &session.owner(sql).await, b'I');
    }
    let (open, recovery_status) = if explicit {
        assert_clean(&label, &session.owner("BEGIN").await, b'T');
        assert_clean(&label, &session.owner("SAVEPOINT before_ddl").await, b'T');
        (b'E', b'T')
    } else {
        (b'I', b'I')
    };

    if wire_kind == Wire::Extended {
        // Parse and Bind finish before the cancellation boundary is armed; an
        // implicit segment keeps its portal open until the Execute and Sync.
        session
            .owner_parse_bind_open("doomed_ddl", form.doomed)
            .await;
    }
    let (canceled, during) = session
        .cancel_at_sleep(doomed_frames(wire_kind, form.doomed), form.probe)
        .await;

    assert_canceled(&label, &canceled, open);
    assert_eq!(scalar(&during).as_deref(), Some(form.before), "{label}");
    if explicit {
        let recovered = session.owner("ROLLBACK TO SAVEPOINT before_ddl").await;
        assert_clean(&label, &recovered, recovery_status);
    }
    assert_eq!(
        scalar(&session.owner(form.probe).await).as_deref(),
        Some(form.before),
        "{label}: owner after recovery"
    );
    assert_eq!(
        scalar(&session.observer(form.probe).await).as_deref(),
        Some(form.before),
        "{label}: observer after recovery"
    );
    for sql in form.retry {
        assert_clean(&label, &session.owner(sql).await, recovery_status);
    }
    if explicit {
        assert_clean(&label, &session.owner("COMMIT").await, b'I');
    }
    assert_eq!(
        scalar(&session.observer(form.probe).await).as_deref(),
        Some(form.after),
        "{label}: observer after retry"
    );
    session.finish().await;
}

fn forms() -> Vec<Form> {
    vec![
        Form {
            name: "CREATE TABLE AS",
            setup: &[],
            doomed: "CREATE TABLE ctas_cancel AS SELECT 1 AS n WHERE \
                     pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL",
            probe: "SELECT count(*) FROM pg_class WHERE relname = 'ctas_cancel'",
            before: "0",
            retry: &["CREATE TABLE ctas_cancel AS SELECT 7 AS n"],
            after: "1",
        },
        Form {
            name: "multi-action ALTER TABLE",
            setup: &[
                "CREATE TABLE multi_cancel (id INT PRIMARY KEY, amount INT)",
                "INSERT INTO multi_cancel VALUES (1, 22)",
            ],
            doomed: "ALTER TABLE multi_cancel ALTER COLUMN amount TYPE NUMERIC(20,2) \
                     USING CASE WHEN pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL \
                     THEN amount::numeric END, ADD COLUMN note TEXT DEFAULT 'added'",
            probe: "SELECT count(*) FROM information_schema.columns \
                    WHERE table_name = 'multi_cancel' AND column_name = 'note'",
            before: "0",
            retry: &[
                "ALTER TABLE multi_cancel ALTER COLUMN amount TYPE NUMERIC(20,2) \
                      USING amount::numeric, ADD COLUMN note TEXT DEFAULT 'added'",
            ],
            after: "1",
        },
        Form {
            name: "ALTER COLUMN TYPE USING",
            setup: &[
                "CREATE TABLE alter_cancel (id INT PRIMARY KEY, amount INT)",
                "INSERT INTO alter_cancel VALUES (1, 22)",
            ],
            doomed: "ALTER TABLE alter_cancel ALTER COLUMN amount TYPE NUMERIC(20,2) \
                     USING CASE WHEN pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL \
                     THEN amount::numeric END",
            probe: "SELECT amount::text FROM alter_cancel",
            before: "22",
            retry: &[
                "ALTER TABLE alter_cancel ALTER COLUMN amount TYPE NUMERIC(20,2) \
                      USING amount::numeric",
            ],
            after: "22.00",
        },
    ]
}

#[test]
fn should_recover_cleanly_after_canceled_selected_ddl_in_every_boundary() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let completed = runtime.block_on(async {
        let mut completed = 0;
        for form in forms() {
            for explicit in [true, false] {
                for wire_kind in [Wire::Simple, Wire::Extended] {
                    run_form(&form, explicit, wire_kind).await;
                    completed += 1;
                }
            }
        }
        completed
    });

    // Assert
    assert_eq!(
        completed,
        forms().len() * 4,
        "every form ran in every boundary"
    );
}
