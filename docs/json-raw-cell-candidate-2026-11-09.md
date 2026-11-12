# Schema-typed raw JSON cell candidate

This test-only #809 candidate explores the selected internal raw-cell design;
it does not change public `Value`, serde output, row-blob, Midge adapters or
runtime dispatch. Jev selected investigation of this design (0.66) with no
production activation; see `json-ordinary-representation-options-2026-11-09.md`
and receipt `target/assembly-review/json-typed-raw-cell-contract-{request,response}.json`.

A cell has an explicit SQL JSON/JSONB identity and a UTF-8 document paired
with its reservation. Its explicit fallible clone independently admits and
copies a second document; each value is destroyed before its own lease. JSON
input retains its raw
spelling; JSONB uses the existing plain-object arena's checked canonical
formatter, including its NUL/numeric errors. The plain parser's inherited depth
limit and diagnostic mapping remain unqualified. Source arena and output
owners overlap during formatting; errors and tracker mismatch must drop
document storage before the corresponding lease.
Jev selected this inline cell and fallible clone (0.86), replacing an Arc
prototype whose reservation dropped inside the Arc before its allocation was
freed. Receipt: `target/assembly-review/json-inline-cell-clone-contract-{request,response}.json`.
The inline cell avoids that allocation; admission for any containing row or
directory and integration with existing infallible clones remain pending.
Allocator transient excess is also pending.
The candidate classifies error categories for probes but does not supply the
production diagnostic owner or preserve its message and SQLSTATE yet.

This is only a local representation experiment. It does not show that
`Value::String` can stand in for a JSON cell, change JSON field bytes or prove
old readers, public serde APIs, query consumers, indexes, column-store paths or
Midge adapter compatibility. The published Midge byte API remains the storage
boundary. Already transformed records are not assumed recoverable. Probes
cover JSON versus JSONB identity/spelling, literal sentinel keys, NULL and
numeric/NUL errors, actual copy interruption, tracker mismatch, measured
exact/below admission and independent-clone release. They remain unexecuted.
