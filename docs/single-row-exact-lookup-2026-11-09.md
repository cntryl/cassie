# Single-entry row exact-name lookup

For a BatchRow with exactly one entry and no aliases, an exact request for its
stored name already resolves to entry zero before any parsed-name or outer-scope
fallback. Return that entry or its corresponding data type directly without
initializing the lazy HashMap. Missing type still returns None without consulting
the outer row. Aliased rows and every nonexact request retain the existing
lookup, candidate-name and outer-scope paths. Exact malformed, quoted and
qualified spellings keep their existing direct-match result.

This source-determined optimization avoids a common untracked lookup allocation
for one-entry JSON object-key output rows. A previously initialized lookup stays
intact; an uninitialized one stays uninitialized, so any existing pending-lookup
admission remains conservative for later nonexact access. String equality and
nonexact HashMap/identifier work are still infallible and uninterruptible, and
no complete lookup/capacity or #809/#810 claim follows. Focused cache, type,
alias and outer-scope probes are unexecuted until the consolidated gate.
