# Controlled JSON existence

This partial #809 increment covers `?`, `?|` and `?&` with a text key or a
native TEXT/NULL array. The source baseline is the tree of `ca4c9077` subject
to assembly of unrelated changes; executable qualification remains deferred.

The current evaluator first parses the entire document as serde `Value`, then
validates the right operand. Objects test top-level keys, arrays test only
string elements and scalar strings can match; numeric, boolean and null
scalars cannot. The native array conversion validates every element before any
match, ignores NULL keys, returns false for empty/all-NULL `?|` and true for
empty/all-NULL `?&`, and then short-circuits as the existing iterator does.
Document parse errors take priority over incompatible right operands.

PostgreSQL 18.6 independently returned true for an escaped duplicate object
key, an array string and a scalar string; false for numeric/null scalars and
`?| ARRAY[NULL]`; and true for `?& ARRAY[NULL]`. PostgreSQL treats the private
serde sentinel spelling as an ordinary key, while Cassie's existing serde
Value parser interprets first-key sentinel objects specially. This increment
preserves Cassie's current Value semantics and error mapping rather than
silently changing that visible behavior. The PostgreSQL-versus-Cassie
private-sentinel discrepancy is an unresolved #809 semantic case: the issue
remains incomplete until its supported input contract is resolved and proved.

Jev selected the owned Value-arena candidate (0.49, legacy 0.43), contingent
on complete Value-mode parsing before RHS inspection, duplicate/sentinel
parity, eager native element validation, and semantic-error fallback to the old
parser. Receipt: `target/assembly-review/json-existence-arena-contract-{request,response}.json`.
The arena owns all parsed strings and object/array indexes through each checked
lookup. Control or budget failures propagate; semantic parse failures and
unsupported operands fall back to the old evaluator, which reparses the input.
String-backed TEXT[] operands remain on that route. Arena admission and
interruption may take precedence over a later legacy error, and semantic
fallback repeats parsing. Diagnostic ownership, allocator transient excess,
arena teardown, JSON function callers and callers lacking controls remain
unqualified.

Authored probes compare legacy results, sentinel and malformed diagnostics,
duplicate/escaped keys, all-NULL/empty/invalid native arrays, short-circuit
boundaries and document-before-operand ordering. They synchronize actual parse
and lookup cancellation and measure exact/below owner overlap. Source review
and Jev do not replace the deferred build, tests or clippy gate.
