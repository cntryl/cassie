# Preserved branch delivery — 2026-11-05

The user requested PR delivery for all 29 remote branches. Inventory at
`a402c722` contained main, 11 archival branches and 17 feature branches.
PRs #1160–#1187 account for the 28 non-main branches. An archive is not a
new feature candidate merely because squash merging left different ancestry.

## Selected bundle

The existing query and transaction histories overlap substantially. Jev selected
one frozen integration candidate over 17 independent restacks (probability
0.99). PR #1184 owns the combined qualification. Other branch records may close
as superseded only after their distinct changes are delivered or are shown
already present in a merged PR. This is a delivery bundle, not a product-scope
expansion or blanket closure of the linked milestone issues.

The bundle retains current main's Midge 0.5.2 dependency, scalar corrections,
startup DDL ownership, explicit NULL parsing and CTAS completion behavior. It
combines admitted array/series sources, character array coercion, pattern
predicates, JSON conversion/containment and semantic keys, statistical FILTER
windows, typed SEARCH/CYCLE lineage, retained catalogs, repeatable-read command
views and row validation, storage compatibility and recovery admission.

Midge remains the direct and only storage layer. No engine source or CI,
release or version configuration is changed. The selected finite contracts in
the type, source, JSON qualification, transaction and storage compatibility
documents remain authoritative. Open acceptance criteria in #813, #815, #817,
#819, #833, #839 and #954 remain open until independently proved.

## Evidence status

The first combined build exposed a duplicate JSON diagnostic helper and a
builtin return-type metadata mismatch. Both are corrected. The runtime build
then passed. Initial test compilation exposed a missing planner import,
`QueryResult::rows()` call sites and Midge `Bytes` adaptation in preserved probes;
these are test compilation failures, not observed assertion baseline failures.

Focused Jev review returned gap probabilities 0.14 for lineage equality, 0.18
for retained key admission, and 0.12 for window FILTER frame membership. A
second review returned 0.14 for unknown-outcome publication fencing, 0.13 for
catalog delta overwrite, 0.13 for catalog admission, and 0.08 for native writer
escape. Its paired RR snapshot concern was 0.60 and requires a probe.
`should_capture_matching_assertion_and_read_views_while_a_publisher_waits`
synchronizes on the competing commit's actual publication gate, captures the
assertion/read pair, and checks historical visibility plus conflict rejection.
The probe and full suite are pending; these judgments do not prove correctness.

The final ordered gate is build, full locked tests, full strict pedantic Clippy,
formatting and touched-test policy validation. Exact-head hosted checks and
merge readback remain separate requirements. The PR review record owns final
results; no pending or historical result is reported as a current pass here.
