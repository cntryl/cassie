# Support, Release, and Security-Response Policy

This policy defines the Cassie pre-release service envelope. It does not promote Experimental
features or external providers to compatibility commitments.

## Supported release boundary

- A supported release is a tagged Cassie artifact with its exact commit, Cargo lockfile, Rust
  toolchain, Midge revision, container digest, and generated Admin UI assets retained together.
- The support envelope is the capabilities marked `Stable` in [Feature Support](feature-support.md).
  Experimental capabilities are evaluation surfaces and may change without compatibility guarantees.
- PostgreSQL wire is the primary interface; REST remains secondary and administrative. Unsupported
  PostgreSQL parity, distributed behavior, trigger/application logic, and provider guarantees remain
  outside the support envelope.

## Upgrade and rollback

- Before upgrading, retain a verified snapshot and the release manifest, then rehearse restore to a
  new data directory using [Snapshot, Backup, Restore, and Repair](snapshot-restore.md).
- A release may be upgraded only when its declared snapshot, catalog, and persistent derived-state
  compatibility contracts match. Cassie rejects incompatible snapshot/layout markers; operators must
  restore with the compatible release or use the documented rebuild path.
- Roll back by stopping writers, preserving the failed data directory and diagnostics, restoring the
  last verified snapshot to a new directory, and routing traffic externally to the known-good release.
  Never overwrite the only copy of the failed directory during incident collection.
- Projection or sidecar rollback follows [Projection Repair Runbook](projection-repair-runbook.md);
  repair and activation remain local, verified, and generation-fenced.

## Incident and security response

| Level | Trigger | Response expectation |
| --- | --- | --- |
| Critical | Data loss/corruption, authentication bypass, remote code execution, or an unsafe release rollback. | Stop promotion, preserve evidence, isolate affected nodes, publish an owner and mitigation immediately, and prepare an emergency patch or rollback. |
| High | Material availability, authorization, query-isolation, or recovery defect without a confirmed data-loss path. | Triage promptly, reproduce against the exact release artifact, document workaround/rollback, and schedule a patch release. |
| Normal | Compatibility, diagnostics, performance, or documentation defect within the supported envelope. | Record the affected release/profile, add regression coverage, and schedule through the normal backlog. |

Security reports should include the affected release/commit, deployment profile, reproduction or
impact, logs stripped of secrets, and whether data or credentials may be exposed. Do not include
passwords, tokens, private keys, or customer data in public issues. Route private disclosures through
the repository's configured private security-reporting channel; publish details only after a fix or
mitigation and affected-release guidance are available.

## Ownership and evidence

The release owner verifies the manifest, compatibility notes, migration/format boundaries, and
rollback rehearsal. The subsystem owner supplies regression tests and recovery diagnostics. The
security owner coordinates disclosure and patch notes. Every incident record links the exact release
artifact, profile, commands, observed diagnostics, decision, and follow-up issue.
