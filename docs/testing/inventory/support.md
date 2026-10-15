# support test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

`support` is a placement group, not a Cargo target. Helper tests compile inside the Cargo suites listed in each inventory record's `instances` field.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `tests::should_report_retrieval_directory_cleanup_failure_on_normal_drop` | [tests/support/retrieval_directory.rs:62](../../../tests/support/retrieval_directory.rs#L62) | `tests/support/retrieval_directory.rs` | registered | no | 1 | recommended |
| `tests::should_preserve_retrieval_cleanup_unwind_panic` | [tests/support/retrieval_directory.rs:81](../../../tests/support/retrieval_directory.rs#L81) | `tests/support/retrieval_directory.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_drop_sql_fixture_owners_before_removing_directory` | [tests/support/sql_fixture.rs:136](../../../tests/support/sql_fixture.rs#L136) | `tests/support/sql_fixture.rs` | registered | no | 12 | recommended |
| `teardown_tests::should_report_directory_cleanup_failure_on_normal_drop` | [tests/support/sql_fixture.rs:170](../../../tests/support/sql_fixture.rs#L170) | `tests/support/sql_fixture.rs` | registered | no | 12 | recommended |
| `teardown_tests::should_report_cleanup_error_without_replacing_an_existing_panic` | [tests/support/sql_fixture.rs:190](../../../tests/support/sql_fixture.rs#L190) | `tests/support/sql_fixture.rs` | registered | no | 12 | recommended |
| `teardown_tests::should_report_cleanup_error_during_unwind_without_a_subscriber` | [tests/support/sql_fixture.rs:218](../../../tests/support/sql_fixture.rs#L218) | `tests/support/sql_fixture.rs` | registered | no | 12 | recommended |
| `tests::should_keep_a_directory_a_running_test_still_owns` | [tests/support/temp_dirs.rs:71](../../../tests/support/temp_dirs.rs#L71) | `tests/support/temp_dirs.rs` | registered | no | 29 | recommended |
| `tests::should_treat_a_directory_older_than_the_window_as_stale` | [tests/support/temp_dirs.rs:90](../../../tests/support/temp_dirs.rs#L90) | `tests/support/temp_dirs.rs` | registered | no | 29 | recommended |
| `tests::should_sweep_only_once_per_process` | [tests/support/temp_dirs.rs:112](../../../tests/support/temp_dirs.rs#L112) | `tests/support/temp_dirs.rs` | registered | no | 29 | recommended |
