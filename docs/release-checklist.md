# Release Checklist

Use this checklist before approving a Cassie release.

> **Current operations constraint:** The latest repository permission readback
> (2026-10-30) reports `enabled: true`, but `AGENTS.md` still prohibits enabling
> or dispatching workflows until the user explicitly changes that direction. Do
> not dispatch workflows. Required backend and release workflow evidence remains
> unavailable; do not treat it as a pass.

## Build and Test

1. Required local unit, integration, compatibility, browser, and benchmark suites completed
   against the exact release commit.
2. Read back required hosted checks on the exact release commit. Although the repository
   permission currently reports enabled, the active no-dispatch instruction remains in force;
   record unrun backend and release workflows as blocked and do not claim a hosted release pass.
3. No unresolved critical or high-severity regressions remain.

## Documentation

1. [Documentation Index](README.md) links are valid.
2. SQL, pgwire, REST, Admin UI, and operational behavior changes are documented.
3. Migration notes identify any wire, snapshot, storage-layout, or derived-state compatibility impact.
4. The supported client and deployment profiles were validated against the release commit.

## Operations Readiness

1. [Cross-Architecture Release Rehearsal](cross-architecture-rehearsal.md) is current.
2. [Snapshot and Restore](snapshot-restore.md) and rollback behavior were rehearsed.
3. Alerts, diagnostics, capacity limits, and recovery guidance match the release.
4. The exact Cargo lockfile, Midge version, container digest, and generated Admin UI are retained.

## Sign-off

1. Engineering sign-off.
2. Operations sign-off.
3. Security sign-off for authentication, authorization, TLS, or policy changes.

## Publish

These steps remain blocked until the user explicitly authorizes workflow execution. The current
repository permission readback reports Actions enabled, but that setting does not override the
no-dispatch instruction.

1. Dispatch the `Publish` workflow from `main` only. Its called `Containers` workflow uses
   `version.yml` to calculate the release SemVer and refuses to continue if the corresponding
   `v<semver>` repository tag already exists.
2. Confirm the called `Containers` workflow published the multi-architecture
   `ghcr.io/cntryl/cassie:<semver>` manifest. That tag is immutable: publishing different image
   content under the same SemVer fails.
3. Confirm the workflow created the annotated `v<semver>` repository tag only after the container
   manifest succeeded. A failed container build does not tag the source commit.
4. For rollback, deploy the previous immutable SemVer image. Do not move or replace an existing
   image or repository version tag.

Use the standalone `Containers` workflow for prerelease branch images. It publishes their
SemVer prerelease without creating repository release tags.
