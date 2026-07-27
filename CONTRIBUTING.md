# Contributing to CX

CX is deliberately conservative: it executes real commands, preserves real exit
codes, and compacts output only when required evidence remains. Contributions
should strengthen that contract rather than broaden the CLI through generic
proxy behavior.

## Setup

Requirements:

- macOS or Linux
- Rust 1.86 or newer
- the native tools needed by the command family you are changing

```sh
git clone https://github.com/contextlimit/cx.git
cd cx
cargo test
```

Use fake binaries and temporary homes for command behavior. Do not run tests
against a contributor's real Docker daemon, Kubernetes cluster, Git history, or
`~/.cx/db.sqlite`.

## Start With A Focused Change

Good contribution shapes include:

- one parser regression from a real command shape;
- one evidence-retention fix;
- one failure artifact or redaction fix;
- one insights query, export, or schema correction;
- one official command-family addition with complete proof;
- one documentation contract backed by a test.

Avoid unrelated refactors in the same pull request. The runner, grep dialect
normalizer, argument preprocessor, exact-output routes, installer, and insights
schema have a large blast radius.

## Official Support Standard

Passthrough is not official support. Promoting a command family requires:

1. A named parser and dispatch route.
2. Natural command-shape coverage, including parser-risky flags.
3. Fake-binary forwarding tests.
4. Output-metric tests when CX changes the output.
5. Evidence-retention assertions.
6. Real exit-code and stderr behavior.
7. Failure artifact coverage.
8. Installed-binary smoke coverage.
9. A feature page linked from `docs/features/index.md`.

Do not add a generic shell proxy, silently run arbitrary command trees through a
shell, or claim that passthrough understands a command.

## Insights And Privacy

Use a temporary database for every test or experiment:

```sh
export CX_INSIGHTS_DB_PATH="$PWD/.tmp/contributor-insights.sqlite"
```

Never delete, migrate, replace, attach, or upload a real
`~/.cx/db.sqlite`. Do not attach real failure artifacts to an issue or pull
request. Use synthetic fixtures with obvious fake secrets.

Changes that store command text, argv, output, paths, report notes, or source
labels must pass through the existing redaction boundary.

## Validation

Run focused tests while developing, then run the release gate:

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- \
  -W clippy::too_many_lines \
  -W clippy::cognitive_complexity
cargo bench --bench cx_hot_paths --no-run
cargo bench --bench cx_iai_hot_paths --no-run
cargo build --release --bin cx
cargo package
```

For install or routing changes:

```sh
./scripts/install.sh
~/.local/bin/cx --version
```

Reinstallation must preserve an existing insights database.

## Code Guidelines

- Keep production process capture on `support::runner`; do not use
  `Command::output()` in production Rust.
- Prefer existing command modules and structured parsers.
- Preserve direct argv execution unless the user explicitly selected `cx sh` or
  a shell command.
- Keep command-family semantics distinct, especially grep, extended grep,
  fixed-string grep, and rg.
- Preserve exact evidence routes and explicit source ranges.
- Keep hand-written Rust files below the repository's 2,000-line test limit.
- Add comments only when the code cannot explain the invariant by structure.

## Pull Requests

The pull request should state:

- the command shape or subsystem changed;
- what native behavior is authoritative;
- what CX emitted before and after;
- which evidence must remain;
- whether output can expand;
- privacy or schema impact;
- focused and full validation commands;
- installed smoke evidence when relevant.

By contributing, you agree that your contribution is licensed under the MIT
License used by this repository.
