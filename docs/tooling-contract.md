# Tooling Contract

The GitHub source pin is the current distribution contract.

Cassie consumes `cntryl-tools` directly from the [cntryl/tools GitHub repository](https://github.com/cntryl/tools)
until that project publishes a release. This is a supported source-pinned dependency, not a
request to publish or install an unpinned branch build.

## UI Toolchain

The UI requires Node.js `^24.15.0 || >=26.0.0` and npm `12.2.1`. This is the
intersection of Cassie's supported Node.js major versions and the pinned npm
release's engine requirement. The UI manifest and lockfile carry the same Node.js
range; the container and frontend CI install the pinned npm version.

## Pinned Source

The backend CI workflow installs the tool with:

```sh
cargo install --git https://github.com/cntryl/tools \
  --rev bc2d07782cf6873e1a4962fe8fe91396daa81a24 \
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
