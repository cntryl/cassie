# Release Checklist

Use this checklist before approving a Cassie release.

> **Current operations constraint:** GitHub Actions are disabled in repository
> settings by user direction. Do not enable or dispatch workflows until the user
> explicitly changes that direction. The Publish and Containers steps below are
> blocked while Actions is disabled; do not treat absent hosted evidence as a
> pass.

## Build and Test

1. Required local unit, integration, compatibility, browser, and benchmark suites completed
   against the exact release commit.
2. Hosted checks are required when Actions are enabled. While Actions remain disabled, record
   hosted evidence as blocked and do not claim a release-ready hosted result.
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

These steps are blocked while GitHub Actions are disabled. Use them only after the user
explicitly reauthorizes workflow execution and the repository settings show Actions enabled.

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
