# support test placements

Generated from `inventory.json`. Selected destinations record the reviewed physical layout; other destinations are proposals. Execution status is recorded separately.

`support` is a placement group, not a Cargo target. Helper tests compile inside the Cargo suites listed in each inventory record's `instances` field.

| Test | Current source | Selected destination | Registration | Ignored | Instances | Placement review |
| --- | --- | --- | --- | --- | ---: | --- |
| `seed_array_tests::should_require_seed_array_shape_with_sql_null_slots` | [tests/support/relational_qualification.rs:208](../../../tests/support/relational_qualification.rs#L208) | `tests/support/relational_qualification.rs` | registered | no | 1 | recommended |
| `should_validate_script_step_error_counts` | [tests/support/repeatable_read.rs:256](../../../tests/support/repeatable_read.rs#L256) | `tests/support/repeatable_read.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_drop_sql_fixture_owners_before_removing_directory` | [tests/support/sql_fixture.rs:191](../../../tests/support/sql_fixture.rs#L191) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_directory_cleanup_failure_on_normal_drop` | [tests/support/sql_fixture.rs:234](../../../tests/support/sql_fixture.rs#L234) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_cleanup_error_without_replacing_an_existing_panic` | [tests/support/sql_fixture.rs:254](../../../tests/support/sql_fixture.rs#L254) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `teardown_tests::should_report_cleanup_error_during_unwind_without_a_subscriber` | [tests/support/sql_fixture.rs:282](../../../tests/support/sql_fixture.rs#L282) | `tests/support/sql_fixture.rs` | registered | no | 1 | recommended |
| `tests::should_keep_a_directory_a_running_test_still_owns` | [tests/support/temp_dirs.rs:95](../../../tests/support/temp_dirs.rs#L95) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
| `tests::should_treat_a_directory_older_than_the_window_as_stale` | [tests/support/temp_dirs.rs:114](../../../tests/support/temp_dirs.rs#L114) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
| `tests::should_sweep_only_once_per_process` | [tests/support/temp_dirs.rs:136](../../../tests/support/temp_dirs.rs#L136) | `tests/support/temp_dirs.rs` | registered | no | 1 | recommended |
