# Explicit local Cassie construction for persisted-index qualification

Issue #954 requires the full-text cancellation fixture to exercise the local
persisted-index path. The current fixture creates an in-memory Midge instance;
that covers the indexed execution boundary but does not prove disk-backed
persistence. `Cassie::new_with_data_dir_and_config` consults storage environment
configuration and therefore cannot guarantee local mode in a shared test process.

The selected additive Rust API is `Cassie::new_local_with_data_dir_and_config`.
It opens the existing strict local Midge adapter with the supplied directory and
configured database, then uses the existing Cassie initialization path. It
ignores process-wide storage-mode selection; other runtime configuration is
supplied explicitly as usual. It adds no storage layer, engine change, durable
format, or implicit global environment mutation. Midge 0.5.2 exposes the local
open operation through `OpenOptions::local`, already used by Cassie's strict
adapter constructor. Jev selected this option at 1.00 against a subprocess-env
alternative; it is an advisory contract choice, not proof.

The full-text cancellation fixture must use an isolated disk directory and
retain the exact scored-candidate synchronization, cancellation, cleanup and
same-session recovery assertions. An explicit local restart witness covers the
constructor and persisted index across reopen. Both are authored source probes;
execution, strict checks and #954 closure remain deferred until the combined
milestone gate. Other ephemeral fixtures remain in memory.
