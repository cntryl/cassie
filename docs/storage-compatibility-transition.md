# Storage compatibility transition — superseded

Status: **superseded on 2026-11-10** by the pre-release policy in
[Release Baseline](release-baseline.md#pre-release-format-and-api-policy-2026-11-10).

Cassie is unreleased. Earlier Cassie storage formats, snapshots and Rust API
shapes are not supported compatibility inputs. Do not implement this document's
v0.1-to-v0.2 marker transition, minimum-reader barrier, old-reader fencing,
in-place migration, rollback or downgrade proof. Use a fresh storage directory
when a pre-release format changes.

The historical design below this note has been removed from the active contract.
Current-build validation, restart/recovery and image round trips remain required.
Midge's own database and SST compatibility is outside Cassie's format policy.
