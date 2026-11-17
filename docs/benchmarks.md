# Benchmark navigation

The [performance contracts](performance-contracts.md) own benchmark scope and
acceptance rules. The [local replay driver](../benches/tier3_replay_durability.md)
records write/clean-reopen diagnostics; the separately selected
[projection replay profiles](projection-replay-profiles.md) define the finite
read campaign, source-copy ownership, execution-path attestation and operator
interpretation. Diagnostic profiles and counters are not durability receipts.
