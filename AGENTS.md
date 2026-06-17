# Cassie Agent Guidelines

## Operating Rules

- Start with TDD for every behavioral change.
- Write the smallest failing test first, then implement only enough code to make it pass.
- Keep tests focused on a single behavior. If a test reads like it checks two things, split it.
- Prefer deterministic, isolated tests with no hidden ordering, network, or time dependencies.
- Keep integration tests at the repository root in `tests/`, and keep module tests close to the code they cover.

## Test Shape

- Each test should describe one observable outcome.
- One test may contain multiple assertions only when they all verify the same behavior.
- Name tests with a `should_` prefix so the intent is obvious and machine-checkable.
- Use descriptive names that make the behavior obvious without reading the body.
- Avoid broad "happy path plus edge cases" tests; split them into separate cases.
- Use explicit `// Arrange`, `// Act`, and `// Assert` comments in each test.
- If a test needs async work, keep the outer test as `#[test]` and drive the async code from a runtime inside it so `cntryl-tools` can validate it.

## Validation

- Use `cntryl-tools validate-tests -f <path>` on any new or edited test file.
- Treat validator failures as a signal to split or simplify tests before moving on.
- If a test cannot be validated as a single behavior, rewrite it until it can.
- Keep the validator clean before considering a test file done.

## Workflow

1. Write a failing test.
2. Make the smallest code change needed to pass.
3. Refactor without broadening the test scope.
4. Run the relevant Rust tests.
5. Run `cntryl-tools validate-tests` on the touched test file or files.

## Cassie Boundaries

- Keep Midge as the direct storage layer for V1.
- Do not introduce a second storage abstraction.
- Keep PostgreSQL wire protocol as the primary query interface.
- Keep REST secondary and administrative.
