//! Every selected #826/#839 DDL form publishes and rolls back through Parse/Bind/Execute.

use crate::support_pgwire as wire;
use crate::support_pgwire_cancel_session::{command_tag, ready_status, scalar, CancelSession};

#[path = "extended_ddl_form_cases.rs"]
mod cases;

use cases::{Form, FORMS};

fn tag_matches(tag: Option<String>, expected: &str, label: &str) {
    let tag = tag.unwrap_or_else(|| panic!("{label}: no CommandComplete"));
    assert!(
        tag.starts_with(expected),
        "{label}: command tag {tag:?} should start with {expected:?}"
    );
}

async fn prepared_session(form: &Form) -> CancelSession {
    let mut session = CancelSession::start().await;
    for sql in form.setup {
        let frames = session.owner(sql).await;
        assert_eq!(
            wire::error_code(&frames),
            None,
            "{}: setup {sql} failed",
            form.name
        );
    }
    assert_eq!(
        scalar(&session.observer(form.probe).await).as_deref(),
        Some(form.before),
        "{}: probe before",
        form.name
    );
    session
}

/// Explicit block: private until COMMIT, discarded by ROLLBACK, re-run and published.
async fn run_explicit(form: &Form) {
    let label = format!("{} (explicit)", form.name);
    let mut session = prepared_session(form).await;
    if !form.explicit {
        // Unselected for staging: the block rejects it deterministically and recovers.
        assert_eq!(ready_status(&session.owner("BEGIN").await), Some(b'T'));
        let rejected = session.owner_prepare("form_ddl_rejected", form.ddl).await;
        assert_eq!(
            wire::error_code(&rejected).as_deref(),
            Some("0A000"),
            "{label}"
        );
        assert_eq!(ready_status(&session.owner("ROLLBACK").await), Some(b'I'));
        assert_eq!(
            scalar(&session.owner(form.probe).await).as_deref(),
            Some(form.before),
            "{label}: rejected form changed nothing"
        );
        session.finish().await;
        return;
    }
    for attempt in ["ROLLBACK", "COMMIT"] {
        assert_eq!(ready_status(&session.owner("BEGIN").await), Some(b'T'));
        let name = format!("form_ddl_{}", attempt.to_lowercase());
        let prepared = session.owner_prepare(&name, form.ddl).await;
        assert_eq!(wire::error_code(&prepared), None, "{label}: prepare");
        let executed = session.owner_execute(&name).await;
        assert_eq!(wire::error_code(&executed), None, "{label}: execute");
        assert_eq!(ready_status(&executed), Some(b'T'), "{label}: block status");
        tag_matches(command_tag(&executed), form.tag, &label);
        assert_eq!(
            scalar(&session.owner(form.probe).await).as_deref(),
            Some(form.after),
            "{label}: owner sees its staged change"
        );
        assert_eq!(
            scalar(&session.observer(form.probe).await).as_deref(),
            Some(form.before),
            "{label}: observer is isolated before {attempt}"
        );
        let finished = session.owner(attempt).await;
        assert_eq!(ready_status(&finished), Some(b'I'), "{label}: {attempt}");
        let expected = if attempt == "COMMIT" {
            form.after
        } else {
            form.before
        };
        for (who, frames) in [
            ("owner", session.owner(form.probe).await),
            ("observer", session.observer(form.probe).await),
        ] {
            assert_eq!(
                scalar(&frames).as_deref(),
                Some(expected),
                "{label}: {who} after {attempt}"
            );
        }
    }
    session.finish().await;
}

/// Implicit Parse/Bind/Execute/Sync publishes before the cycle returns ready.
async fn run_implicit(form: &Form) {
    let label = format!("{} (implicit)", form.name);
    let mut session = prepared_session(form).await;
    let executed = session
        .owner_frames(vec![
            wire::parse_frame("form_ddl", form.ddl),
            wire::bind_frame("form_portal", "form_ddl", &[]),
            wire::execute_frame("form_portal"),
            wire::sync_frame(),
        ])
        .await;
    assert_eq!(wire::error_code(&executed), None, "{label}: execute");
    assert_eq!(ready_status(&executed), Some(b'I'), "{label}: ready status");
    tag_matches(command_tag(&executed), form.tag, &label);
    assert_eq!(
        scalar(&session.observer(form.probe).await).as_deref(),
        Some(form.after),
        "{label}: observer sees the published change"
    );
    session.finish().await;
}

#[test]
fn should_publish_every_selected_ddl_form_through_extended_protocol() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let completed = runtime.block_on(async {
        let mut completed = 0;
        for form in FORMS {
            run_explicit(form).await;
            run_implicit(form).await;
            completed += 1;
        }
        completed
    });

    // Assert
    assert_eq!(completed, FORMS.len(), "every form ran in both boundaries");
}
