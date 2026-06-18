# Cassie V1 Sprint Roadmap

This roadmap breaks Cassie V1 into thirty-nine implementation sprints. The first thirty-six are complete; sprints 37-39 are the next backlog slice. The target is a single-container, Midge-direct query engine with real PostgreSQL binary wire compatibility for practical clients such as `psql`, libpq, common language drivers, ORMs, migration tools, and BI tools.

Cassie is PostgreSQL-wire compatible, but it is not PostgreSQL. Unsupported PostgreSQL features must return deterministic PostgreSQL-style errors rather than partial or surprising behavior.

## Sprint Order

1. [Sprint 01 - Foundation, Repo Contract, Runtime Baseline](completed/sprint-01.md)
2. [Sprint 02 - Midge Storage Contract and Catalog Hydration](completed/sprint-02.md)
3. [Sprint 03 - SQL Parser and Binder V1](completed/sprint-03.md)
4. [Sprint 04 - Planner, Optimizer, and Physical Plan Determinism](completed/sprint-04.md)
5. [Sprint 05 - Executor Semantics and Query Result Contract](completed/sprint-05.md)
6. [Sprint 06 - Common Table Expressions](completed/sprint-06.md)
7. [Sprint 07 - Schema Objects and DDL Compatibility](completed/sprint-07.md)
8. [Sprint 08 - Indexes and Constraints](completed/sprint-08.md)
9. [Sprint 09 - UDFs and Stored Procedures](completed/sprint-09.md)
10. [Sprint 10 - Full-Text Search Stack](completed/sprint-10.md)
11. [Sprint 11 - Vector and Hybrid Retrieval](completed/sprint-11.md)
12. [Sprint 12 - Runtime Observability, Plan Cache, and Operational Controls](completed/sprint-12.md)
13. [Sprint 13 - Row Blob Persistence Core](completed/sprint-13.md)
14. [Sprint 14 - Row Storage Rebuild and Decode Controls](completed/sprint-14.md)
15. [Sprint 15 - SQL INSERT VALUES](completed/sprint-15.md)
16. [Sprint 16 - SQL INSERT SELECT](completed/sprint-16.md)
17. [Sprint 17 - SQL UPDATE](completed/sprint-17.md)
18. [Sprint 18 - SQL DELETE](completed/sprint-18.md)
19. [Sprint 19 - Transaction Control Basics](completed/sprint-19.md)
20. [Sprint 20 - Transaction Write Semantics](completed/sprint-20.md)
21. [Sprint 21 - Relational Predicates and Scalar SQL](completed/sprint-21.md)
22. [Sprint 22 - Joins and FROM Subqueries](completed/sprint-22.md)
23. [Sprint 23 - Aggregates, DISTINCT, and Set Operations](completed/sprint-23.md)
24. [Sprint 24 - PostgreSQL Catalog Basics](completed/sprint-24.md)
25. [Sprint 25 - Catalog Compatibility Probes](completed/sprint-25.md)
26. [Sprint 26 - Type Catalog and SQL Casts](completed/sprint-26.md)
27. [Sprint 27 - Wire Type Metadata Policy](completed/sprint-27.md)
28. [Sprint 28 - Auth and Session Identity](completed/sprint-28.md)
29. [Sprint 29 - Binary Pgwire Startup and Auth](completed/sprint-29.md)
30. [Sprint 30 - Binary Pgwire Simple Query](completed/sprint-30.md)
31. [Sprint 31 - Extended Query Parse/Bind/Execute](completed/sprint-31.md)
32. [Sprint 32 - Extended Query Portals and Recovery](completed/sprint-32.md)
33. [Sprint 33 - Compatibility Matrix and CI Gate](completed/sprint-33.md)
34. [Sprint 34 - REST, Operations, Packaging, and V1 Release Gate](completed/sprint-34.md)

## Planned Backlog

35. [Sprint 35 - User-Defined Views and View Expansion](completed/sprint-35.md)
36. [Sprint 36 - Stored Procedure Execution and CALL Semantics](completed/sprint-36.md)
37. [Sprint 37 - Common Scalar Functions](sprint-37.md)
38. [Sprint 38 - SQL Type Coverage and Metadata Fidelity](sprint-38.md)
39. [Sprint 39 - Schema DDL Breadth and Index Variants](sprint-39.md)

## Shared Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` rows/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## How To Use This Roadmap

Each sprint is intentionally small and closeable. Start at the top of the sprint file, write the first failing behavior test, implement the smallest passing change, validate touched tests with `cntryl-tools`, and stop only when the sprint exit gate is green.

Do not skip ahead to protocol or feature polish while earlier storage, SQL, planner, and executor invariants are failing. PostgreSQL compatibility depends on the lower query stack behaving deterministically.

## Sprint Completion Steps

- Close a finished sprint by moving its file from `docs/roadmap/sprint-XX.md` to `docs/roadmap/completed/sprint-XX.md`.
- Update `docs/roadmap/README.md` to point that sprint entry at `completed/sprint-XX.md`.
- Update the `Next` link in the previous sprint file to point to the completed file.
- Update the `Previous` link in the next sprint file, if not completed, to point to `completed/sprint-XX.md`.
- Run the sprint exit gates before finalizing and committing.

## Required Gates

Every sprint must end with:

- Targeted sprint tests passing.
- `cntryl-tools validate-tests -f <file>` passing for every touched test file.
- `cargo build` passing.
- `cargo clippy --all-targets --all-features -- -D warnings` passing.
- Full `cargo test` passing for storage, executor, transaction, wire, release-gate, and shared runtime behavior changes.
