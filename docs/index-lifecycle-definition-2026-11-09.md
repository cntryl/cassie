# Index definition guards during lifecycle changes

The index definition readback guard applies to `get_index` and `list_indexes`,
but rename and metadata update read the stored record directly. Those paths
must apply the same definition/version guard before using a decoded record.
Rename keeps its existing missing-source and destination-collision precedence.
Metadata update validates its final replacement before persisting or calling
its catalog publication callback. Its existing update error and identity checks
precede the final replacement check. This private operation preserves the stored
definition version: its sole production caller changes storage options, while
normal index publication owns version transitions and reader-layout admission.
It does not acquire the schema marker lock inside the data transaction.
An already-stored version 1 record is treated consistently with the existing
readback path; these checks do not prove that its layout marker was previously
admitted or repair externally written metadata.

These checks reject unsafe encoded index subqueries and unknown definition
versions without altering the stored record or publishing a catalog update.
Malformed records retain the existing parse error. Valid old metadata remains
eligible for rename and updates. This changes rejection timing for invalid
records at these direct lifecycle boundaries; it does not change the metadata
format, use another storage layer, or claim old-reader compatibility.

Focused in-memory regression probes are authored but unexecuted. The consolidated
build, tests, pedantic Clippy and remaining ordered gates are deferred.
