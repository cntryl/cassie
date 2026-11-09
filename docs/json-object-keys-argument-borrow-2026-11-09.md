# Borrow direct JSON object-key arguments

For the existing builtin bare `json_object_keys` and `jsonb_object_keys` with
exactly one direct Column or Param argument, statement controls, an expression
owner and no general owner, borrow that argument from the row/local arguments
or parameter slice through checked inspection and output-owner adoption. At
the original nested argument boundary, invoke session statement-text scratch
and leaf admission in their existing order; the scratch is dropped before
builtin dispatch. Missing columns/parameters borrow a local SQL NULL. The
regex, JSON builder, array and other builtin dispatch paths cannot select these
names; static JSON builtin dispatch precedes user-defined fallback. Other
names, qualified calls, complex arguments, general-owner and no-controls paths
retain the original owned-argument route.

Jev selected this scope (0.93). The 0.81 ordering/ownership lead requires
actual clone-elimination, session scratch, local-argument-before-row, NULL,
user override, foreign tracker, interruption and source/output lifetime probes.
Receipt `target/assembly-review/date-and-object-keys-argument-{request,response}.json`.
The source row/parameter remains borrowed until adoption, but its prior storage
or construction ownership is not established here. Selected work still has
preflight/scratch admission, expansion and teardown limits, and an unselected
argument or later scalar conversion may clone. Tests and Cargo checks remain
unexecuted until the consolidated gate.
