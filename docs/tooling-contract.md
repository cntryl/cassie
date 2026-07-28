# Tooling Contract

The GitHub source pin is the current distribution contract.

Cassie consumes `cntryl-tools` directly from the [cntryl/tools GitHub repository](https://github.com/cntryl/tools)
until that project publishes a release. This is a supported source-pinned dependency, not a
request to publish or install an unpinned branch build.

## Pinned Source

CI and benchmark workflows install the tool with:

```sh
cargo install --git https://github.com/cntryl/tools \
  --rev b2a06b1a635de752803f5860339fc3cecbc19742 \
  --locked cntryl-tools
```

The revision is explicit so hosted validation is reproducible. Updating it requires rerunning
the Cassie validation sequence and reviewing parity for `validate-tests`; no published crate or
release tag is required for this contract.

## Boundary

The source-pinned tool owns generic test-file validation. Cassie continues to own its repository
instructions, subsystem-specific commands, benchmark evidence contracts, and acceptance criteria.
Changing the pin does not imply that a complete benchmark, production profile, or external client
compatibility claim has been observed.
