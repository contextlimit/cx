# CX Cargo Test Features

CX supports Cargo through the explicit `cargo test` command family. The wrapper
is designed for proof commands: preserve real Cargo semantics and exit codes,
reduce routine output volume, and keep the diagnostics needed to fix compile or
test failures.

The supported surface is:

```sh
cx cargo test <args...>
cx -- cargo test <args...>
```

Unsupported Cargo subcommands are not official CX command families. In
environments where unsupported passthrough is enabled, `cx -- cargo <args...>`
may still run as passthrough and record opportunity telemetry, but that is
separate from official `cargo test` support.

## Normal Cargo Test

The normal shape:

```sh
cx cargo test
```

converts to:

```sh
cargo test
```

User arguments are forwarded:

```sh
cx cargo test -p cx --lib
```

converts to:

```sh
cargo test -p cx --lib
```

CX preserves Cargo's real exit code. Output is filtered so successful proof runs
stay bounded and failures keep the evidence needed for repair.

Failure evidence is expected to preserve:

- Rust compiler error codes such as `E0425`
- file and line locations
- primary diagnostic messages
- panic lines
- failing test names
- final `test result:` lines
- relevant build or compile failure context

CX does not treat the filtered output as a new source of truth. It is a bounded
rendering of real Cargo output.

## Harness Arguments

Cargo test uses `--` to separate Cargo arguments from test harness arguments.
CX preserves that separator.

Example:

```sh
cx cargo test support::insights_tests -- --exact --nocapture
```

converts to:

```sh
cargo test support::insights_tests -- --exact --nocapture
```

Harness arguments after `--` are not interpreted as Cargo package or target
selectors. They are passed to the test harness.

## Multi-Filter Repair

Cargo accepts one positional test-name filter before `--`. Agents often produce
commands that include several test names in one command:

```sh
cx cargo test test_a test_b test_c -- --exact
```

Native Cargo does not treat that as three exact test filters. CX repairs safe
multi-filter shapes by splitting them into multiple valid Cargo invocations.

The example above becomes three sequential child commands:

```sh
cargo test test_a -- --exact
cargo test test_b -- --exact
cargo test test_c -- --exact
```

CX emits a parent summary before the child sections:

```text
cargo test: split 3 filters into 3 cargo test runs
```

Each child section is labeled with its filter and exit code:

```text
[1/3] test_a (exit 0)
[2/3] test_b (exit 0)
[3/3] test_c (exit 0)
```

The overall CX exit code is nonzero if any child invocation fails. Successful
children and failing children both contribute evidence to the combined output.

## Prefix Arguments

CX only splits multi-filter commands when it can safely identify arguments that
belong before every child invocation.

These common prefix arguments are preserved for each split child:

- `-p <package>`
- `--package <package>`
- `--workspace`
- `--all`
- `--lib`
- `--bins`
- `--examples`
- `--tests`
- `--benches`
- `--all-targets`
- `--features <features>`
- `--all-features`
- `--no-default-features`
- `--target <triple>`
- `--target-dir <path>`
- `--manifest-path <path>`
- `--jobs <n>`
- `--message-format <format>`
- `--color <when>`
- `--profile <name>`
- `--release`
- verbosity and quiet flags

Example:

```sh
cx cargo test -p clob-engine fee_payout replay_obligation -- --exact
```

converts to:

```sh
cargo test -p clob-engine fee_payout -- --exact
cargo test -p clob-engine replay_obligation -- --exact
```

The package selector stays attached to both child commands.

## Ambiguous Shapes

CX only repairs shapes that are clearly safe. If a command contains ambiguous
options after a positional filter, CX does not guess. It falls back to a single
native Cargo invocation.

The reason is simple: a repair that runs a different test set is worse than a
native Cargo error. CX should reduce common command mistakes, not invent Cargo
semantics.

When split repair is not applied, the command remains:

```sh
cargo test <original args...>
```

and the filtered output explains the actual Cargo result.

## Output Contract

For compile failures, CX keeps the compiler diagnostics needed to fix the code.
For test failures, CX keeps the failing test names, panic or assertion evidence,
and final test summary lines. For success, CX keeps enough proof to show that the
requested tests ran without printing every routine build line.

The wrapper may reduce:

- repeated compile progress lines
- long successful test listings
- duplicate warnings
- noisy success output

The wrapper must not remove:

- the first useful compiler diagnostic
- error codes and source locations
- failing test names
- assertion messages
- panic locations
- final pass/fail result summaries

## Insights Labels

When insights recording is enabled, Cargo test invocations are recorded with
command identity that can be aggregated later:

- process: `cargo`
- command family: `cargo test`
- command: readable redacted command text
- observation source: `cargo test` or `cargo test split-filters`

The `cargo test split-filters` source means CX repaired a safe multi-filter shape
into multiple valid Cargo test runs.

Useful analysis questions include:

- how often Cargo proof commands run
- how many bytes and tokens Cargo filtering saves
- how often multi-filter repair prevents a malformed proof command
- which package selectors appear most often
- whether failures are compile failures, test failures, or command-shape errors

## Command Selection Guide

Use `cx cargo test` for normal Rust proof.

Use `cx cargo test <filter> -- --exact` for one exact test filter.

Use `cx cargo test <filter-a> <filter-b> -- --exact` when the intended action is
to run several exact filters and let CX split them into valid child invocations.

Use separate explicit commands when filters need different package selectors,
features, harness arguments, or environment.

Use raw Cargo only when CX is unavailable or when you need a Cargo subcommand
outside official CX support.
