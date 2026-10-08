# Local test fixture ownership and cleanup

The selected #782 harness bundle covers `SqlFixture` and
`metrics_adaptive::should_report_operator_switch_failure_without_claiming_success`,
and the typed-parameter fixture in `tests/parser_types.rs`.
It preserves local storage, SQL inputs, configuration and correctness assertions.
It does not change production storage, shutdown deadlines, runners or required
checks, and does not convert other metrics fixtures.

## Destruction before directory removal

The metrics fixture explicitly drops its session and Cassie before strict
directory removal. `SqlFixture` declares its session, Cassie and private directory
guard in that order. Rust drops those fields in declaration order after its
custom Drop body. The intentionally empty `SqlFixture::drop` retains the existing
prohibition on moving its public owning fields out of the fixture.

The directory guard removes the path only after the fixture's owners finish
dropping. Cleanup errors fail visibly on normal destruction. During an existing
unwind, it reports the cleanup failure directly to stderr and through tracing
without replacing the original panic. Diagnostic I/O is best effort so a write
failure cannot cause a second panic.

Callers must finish any separately cloned Cassie, server or task owners before
dropping a fixture. The audited current callers do not export such owners. This
guard controls the fixture's fields; it cannot close an escaped external owner.

The typed-parameter fixture uses a fresh local UUID directory and completes
session and Cassie destruction before strict cleanup. Its former process-global
memory setter could displace a generic fixture between local-mode selection and
engine construction in the same test executable. A composed source probe ran all
six original typed-parameter assertions in that constructor window: the old
fixture left the generic local path absent, while the selected local fixture
preserved the path and strict cleanup. This demonstrates the controlled mechanism,
not a particular historical or hosted scheduling order.

## Finite diagnostic evidence

At main `7977c848775ff4b776dbf40bda2bbb9893aac25d`, five measured matched pairs
for each selected fixture preserved query outputs, resource metrics and directory
cleanup. Removing the directory first produced missing-leader fencing errors and
a Midge graceful-shutdown timeout in every old-order run. Completing owner drop
first avoided those errors in every corresponding control. The metrics probe
attributed approximately five seconds to owner destruction, with query work
around five milliseconds. Startup, servers and guard waits were absent there;
the generic fixture regression separately covers callers with and without startup.

The cached diagnostic binary was compiled at
`cbd5267ed58db5a289edd2562e824008e6f4c35c`; its runtime, test and build inputs
were verified byte-equivalent to the main revision. Its compiled paths were
retained. Concurrent contract validation prevents treating these observations as
an isolated-host performance benchmark or a whole-suite speedup. They establish
the selected teardown mechanism, not a production latency regression or SLA.

## Regression and validation boundaries

The scoped warning regressions first failed against the original owning seams.
They preserve every selected SQL assertion and check that fixture teardown does
not emit the graceful-shutdown timeout. The capture helper uses a thread-scoped
subscriber, never installs a global subscriber, and makes no wallclock assertion.
Cleanup controls verify path removal, visible normal failures, preservation of
an existing panic, and stderr reporting without a configured subscriber. The
last control runs in an isolated child process with a private test selector;
it does not change the parent's environment or application configuration.

Focused controls and these measurements do not replace complete normal-threaded
tests, broad pedantic Clippy, formatting, touched-test policies, documentation and
benchmark checks, exact-head hosted checks or merge/readback. No issue closure or
native operational promotion follows from local fixture attribution alone.
