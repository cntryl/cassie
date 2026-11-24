use super::*;
use crate::config::CassieRuntimeConfig;

#[test]
fn should_keep_the_replacement_healthy_after_a_copy_worker_error() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("copy_outer", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_outer_rows (id INT)", vec![])
        .unwrap();
    session.begin_transaction(None).unwrap();
    let operation = session.with_new_wire_operation();
    let request = request::parse_copy_request("COPY copy_outer_rows (id) FROM STDIN")
        .unwrap()
        .unwrap();
    let BoundCopyRequest::From(request, _statement, binding) =
        bind_copy_from_stdin(&cassie, &operation, request, None).unwrap()
    else {
        panic!("COPY FROM");
    };
    let mut payload = CopyPayload::new(&binding.controls).unwrap();
    payload
        .append(b"not-an-integer\n", &binding.controls)
        .unwrap();
    let _hook = super::super::wire_failure_test_hooks::install(&cassie, |session| {
        let original = session
            .operation_failure_authority()
            .and_then(crate::app::FailureAuthority::identity);
        let independent = session.without_operation_failure_authority();
        independent.rollback_transaction();
        independent.begin_transaction(None).unwrap();
        assert_eq!(
            session
                .operation_failure_authority()
                .and_then(crate::app::FailureAuthority::identity),
            original
        );
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let mut output = Vec::new();
    let handled = runtime.block_on(finish_simple_copy(
        cassie.clone(),
        *binding,
        session.clone(),
        request,
        payload,
        None,
        &mut output,
    ));

    // Assert
    assert!(handled);
    assert!(session.is_transaction_active());
    assert!(!session.is_transaction_failed());
    assert!(cassie.execute_sql(&session, "SELECT 1", vec![]).is_ok());
}
